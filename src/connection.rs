//! WebSocket connection management with auto-reconnect and stream resumption.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use titan_api_codec::codec::ws::v1::ClientCodec;
use titan_api_codec::codec::Codec;
use titan_api_types::ws::v1::{
    ClientRequest, RequestData, ResponseError, ResponseSuccess, ServerMessage, StreamData,
    SwapQuoteRequest,
};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::config::TitanConfig;
use crate::error::TitanClientError;
use crate::state::ConnectionState;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type ResponseResult = Result<ResponseSuccess, ResponseError>;
type PendingRequestsMap = Arc<RwLock<HashMap<u32, oneshot::Sender<ResponseResult>>>>;
type OnEndCallback = Arc<dyn Fn() + Send + Sync>;

/// Initial backoff delay in milliseconds.
pub const INITIAL_BACKOFF_MS: u64 = 100;

/// Default ping interval in milliseconds (used if config value is 0).
pub const DEFAULT_PING_INTERVAL_MS: u64 = 25_000;

/// Default pong timeout in milliseconds — if no pong received within this window, reconnect.
pub const DEFAULT_PONG_TIMEOUT_MS: u64 = 10_000;

/// Information needed to resume a stream after reconnection.
#[derive(Clone)]
pub struct ResumableStream {
    /// The original request used to create the stream.
    pub request: SwapQuoteRequest,
    /// Channel to send stream data to.
    pub sender: mpsc::Sender<StreamData>,
    /// Called when this stream ends (server-initiated or reconnect failure) to release the slot.
    pub on_end: Option<OnEndCallback>,
    /// Shared atomic that tracks the current server-side stream ID (updated on reconnect remap).
    pub effective_id: Option<Arc<AtomicU32>>,
}

type ResumableStreamsMap = Arc<RwLock<HashMap<u32, ResumableStream>>>;

/// Internal message for sending requests through the connection
pub struct PendingRequest {
    pub request: ClientRequest,
    pub response_tx: oneshot::Sender<ResponseResult>,
}

/// Manages a WebSocket connection to the Titan API with auto-reconnect.
pub struct Connection {
    #[expect(dead_code)]
    config: TitanConfig,
    request_id: AtomicU32,
    sender: mpsc::Sender<PendingRequest>,
    state_tx: tokio::sync::watch::Sender<ConnectionState>,
    #[expect(dead_code)]
    pending_requests: PendingRequestsMap,
    resumable_streams: ResumableStreamsMap,
}

impl Connection {
    /// Create a new connection with the given config.
    ///
    /// Connects eagerly and auto-reconnects on disconnection.
    #[tracing::instrument(skip_all)]
    pub async fn connect(config: TitanConfig) -> Result<Self, TitanClientError> {
        let (state_tx, _state_rx) = tokio::sync::watch::channel(ConnectionState::Disconnected {
            reason: "Connecting...".to_string(),
        });

        let pending_requests: PendingRequestsMap = Arc::new(RwLock::new(HashMap::new()));
        let resumable_streams: ResumableStreamsMap = Arc::new(RwLock::new(HashMap::new()));

        // Connect to WebSocket
        let ws_stream = Self::establish_connection(&config).await?;

        // Create channel for sending requests
        let (sender, receiver) = mpsc::channel::<PendingRequest>(32);

        // Spawn background task with reconnection support
        let pending_clone = pending_requests.clone();
        let streams_clone = resumable_streams.clone();
        let state_tx_clone = state_tx.clone();
        let config_clone = config.clone();

        tokio::spawn(Self::run_connection_loop_with_reconnect(
            ws_stream,
            receiver,
            pending_clone,
            streams_clone,
            state_tx_clone,
            config_clone,
        ));

        state_tx.send_replace(ConnectionState::Connected);

        Ok(Self {
            config,
            request_id: AtomicU32::new(1),
            sender,
            state_tx,
            pending_requests,
            resumable_streams,
        })
    }

    /// Establish WebSocket connection with authentication.
    async fn establish_connection(config: &TitanConfig) -> Result<WsStream, TitanClientError> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use tokio_tungstenite::Connector;

        let url = if config.url.contains("/ws") || config.url.ends_with('/') {
            format!("{}?auth={}", config.url, config.token)
        } else {
            format!("{}/?auth={}", config.url, config.token)
        };

        let mut request = url.into_client_request().map_err(|e| {
            TitanClientError::Unexpected(anyhow::anyhow!("Failed to build request: {e}"))
        })?;

        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            titan_api_types::ws::v1::WEBSOCKET_SUBPROTO_BASE
                .parse()
                .map_err(|e| {
                    TitanClientError::Unexpected(anyhow::anyhow!(
                        "Sec-WebSocket-Protocol fail: {e}"
                    ))
                })?,
        );

        let tls_config = if config.danger_accept_invalid_certs {
            crate::tls::build_dangerous_tls_config()
        } else {
            crate::tls::build_default_tls_config()
        }
        .map_err(|e| TitanClientError::Unexpected(anyhow::anyhow!("TLS config failed: {e}")))?;
        let connector = Connector::Rustls(Arc::new(tls_config));
        let (ws_stream, _response) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector))
                .await
                .map_err(TitanClientError::WebSocket)?;
        Ok(ws_stream)
    }

    /// Connection loop with automatic reconnection and stream resumption.
    async fn run_connection_loop_with_reconnect(
        initial_ws_stream: WsStream,
        mut request_rx: mpsc::Receiver<PendingRequest>,
        pending_requests: PendingRequestsMap,
        resumable_streams: ResumableStreamsMap,
        state_tx: tokio::sync::watch::Sender<ConnectionState>,
        config: TitanConfig,
    ) {
        let mut ws_stream = initial_ws_stream;
        let mut reconnect_attempt: u32 = 0;
        let mut request_id_counter: u32 = 1;

        loop {
            // Run the connection loop until disconnection
            let disconnect_reason = Self::run_single_connection(
                &mut ws_stream,
                &mut request_rx,
                &pending_requests,
                &resumable_streams,
                &state_tx,
                &mut request_id_counter,
                &config,
            )
            .await;

            // Check if request channel is closed (client dropped)
            if request_rx.is_closed() {
                tracing::info!("Request channel closed, shutting down connection");
                break;
            }

            // Start reconnection attempts
            reconnect_attempt += 1;

            // Check max attempts
            if let Some(max) = config.max_reconnect_attempts {
                if reconnect_attempt > max {
                    tracing::error!("Max reconnect attempts ({}) reached, giving up", max);
                    let _ = state_tx.send(ConnectionState::Disconnected {
                        reason: format!(
                            "Max reconnect attempts reached. Last error: {}",
                            disconnect_reason
                        ),
                    });
                    break;
                }
            }

            // Calculate backoff delay with exponential increase
            let backoff_ms = calculate_backoff(reconnect_attempt, config.max_reconnect_delay_ms);

            tracing::debug!(
                attempt = reconnect_attempt,
                backoff_ms,
                "Reconnecting after disconnection: {}",
                disconnect_reason
            );

            let _ = state_tx.send(ConnectionState::Reconnecting {
                attempt: reconnect_attempt,
            });

            // Wait before reconnecting
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

            // Attempt to reconnect
            match Self::establish_connection(&config).await {
                Ok(new_stream) => {
                    ws_stream = new_stream;
                    reconnect_attempt = 0;
                    let _ = state_tx.send(ConnectionState::Connected);
                    tracing::debug!("Reconnected successfully");

                    // Resume streams after reconnection
                    Self::resume_streams(
                        &mut ws_stream,
                        &resumable_streams,
                        &mut request_id_counter,
                    )
                    .await;
                }
                Err(e) => {
                    tracing::warn!("Reconnection failed: {}", e);
                }
            }
        }

        // Final cleanup
        Self::cleanup_pending_requests(&pending_requests).await;
        Self::cleanup_resumable_streams(&resumable_streams).await;
    }

    /// Resume all active streams after reconnection.
    async fn resume_streams(
        ws_stream: &mut WsStream,
        resumable_streams: &ResumableStreamsMap,
        request_id_counter: &mut u32,
    ) {
        let streams_to_resume: Vec<(u32, ResumableStream)> = {
            let streams = resumable_streams.read().await;
            streams.iter().map(|(k, v)| (*k, v.clone())).collect()
        };

        if streams_to_resume.is_empty() {
            return;
        }

        tracing::info!(
            "Resuming {} streams after reconnection",
            streams_to_resume.len()
        );

        let codec = ClientCodec::Uncompressed;
        let mut encoder = codec.encoder();
        let mut decoder = codec.decoder();

        for (old_stream_id, resumable) in streams_to_resume {
            let request_id = *request_id_counter;
            *request_id_counter += 1;

            let request = ClientRequest {
                id: request_id,
                data: RequestData::NewSwapQuoteStream(resumable.request.clone()),
            };

            // Encode and send the request
            let encoded = match encoder.encode_mut(&request) {
                Ok(data) => data.to_vec(),
                Err(e) => {
                    tracing::error!("Failed to encode stream resume request: {}", e);
                    continue;
                }
            };

            if let Err(e) = ws_stream.send(Message::Binary(encoded.into())).await {
                tracing::error!("Failed to send stream resume request: {}", e);
                continue;
            }

            // Wait for response to get new stream ID
            match ws_stream.next().await {
                Some(Ok(Message::Binary(data))) => {
                    match decoder.decode_mut(data) {
                        Ok(ServerMessage::Response(response)) => {
                            if let Some(stream_info) = response.stream {
                                let new_stream_id = stream_info.id;

                                // Update the stream mapping
                                let mut streams = resumable_streams.write().await;
                                if let Some(stream) = streams.remove(&old_stream_id) {
                                    // Update the shared effective_id so QuoteStream uses the new ID
                                    if let Some(ref effective_id) = stream.effective_id {
                                        effective_id.store(new_stream_id, Ordering::SeqCst);
                                    }
                                    streams.insert(new_stream_id, stream);
                                    tracing::info!(
                                        old_id = old_stream_id,
                                        new_id = new_stream_id,
                                        "Stream resumed with new ID"
                                    );
                                }
                            }
                        }
                        Ok(ServerMessage::Error(error)) => {
                            tracing::error!(
                                "Failed to resume stream {}: {}",
                                old_stream_id,
                                error.message
                            );
                            // Remove the failed stream and release its slot
                            let mut streams = resumable_streams.write().await;
                            if let Some(stream) = streams.remove(&old_stream_id) {
                                if let Some(ref on_end) = stream.on_end {
                                    on_end();
                                }
                            }
                        }
                        Ok(_) => {
                            tracing::warn!("Unexpected response type during stream resumption");
                        }
                        Err(e) => {
                            tracing::error!("Failed to decode stream resume response: {}", e);
                        }
                    }
                }
                Some(Ok(_)) => {
                    tracing::warn!("Unexpected message type during stream resumption");
                }
                Some(Err(e)) => {
                    tracing::error!("WebSocket error during stream resumption: {}", e);
                    break;
                }
                None => {
                    tracing::error!("Connection closed during stream resumption");
                    break;
                }
            }
        }
    }

    /// Run a single connection until disconnection.
    async fn run_single_connection(
        ws_stream: &mut WsStream,
        request_rx: &mut mpsc::Receiver<PendingRequest>,
        pending_requests: &PendingRequestsMap,
        resumable_streams: &ResumableStreamsMap,
        state_tx: &tokio::sync::watch::Sender<ConnectionState>,
        request_id_counter: &mut u32,
        config: &TitanConfig,
    ) -> String {
        let codec = ClientCodec::Uncompressed;
        let mut encoder = codec.encoder();
        let mut decoder = codec.decoder();

        let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();

        let ping_interval_ms = if config.ping_interval_ms > 0 {
            config.ping_interval_ms
        } else {
            DEFAULT_PING_INTERVAL_MS
        };
        let mut ping_timer = tokio::time::interval(Duration::from_millis(ping_interval_ms));
        ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let pong_timeout = Duration::from_millis(config.pong_timeout_ms);
        let mut last_pong = tokio::time::Instant::now();

        loop {
            tokio::select! {
                Some(pending_req) = request_rx.recv() => {
                    let request_id = pending_req.request.id;
                    *request_id_counter = request_id.max(*request_id_counter) + 1;

                    {
                        let mut pending_map = pending_requests.write().await;
                        pending_map.insert(request_id, pending_req.response_tx);
                    }

                    match encoder.encode_mut(&pending_req.request) {
                        Ok(data) => {
                            if let Err(e) = ws_sink.send(Message::Binary(data.to_vec().into())).await {
                                tracing::error!("Failed to send WebSocket message: {e}");
                                let mut pending_map = pending_requests.write().await;
                                if let Some(tx) = pending_map.remove(&request_id) {
                                    let _ = tx.send(Err(ResponseError {
                                        request_id,
                                        code: 0,
                                        message: format!("Send failed: {e}"),
                                    }));
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to encode request: {e}");
                            let mut pending_map = pending_requests.write().await;
                            if let Some(tx) = pending_map.remove(&request_id) {
                                let _ = tx.send(Err(ResponseError {
                                    request_id,
                                    code: 0,
                                    message: format!("Encode failed: {e}"),
                                }));
                            }
                        }
                    }
                }

                Some(msg_result) = ws_stream_rx.next() => {
                    match msg_result {
                        Ok(Message::Binary(data)) => {
                            match decoder.decode_mut(data) {
                                Ok(server_msg) => {
                                    Self::handle_server_message(
                                        server_msg,
                                        pending_requests,
                                        resumable_streams,
                                    ).await;
                                }
                                Err(e) => {
                                    tracing::error!("Failed to decode server message: {e}");
                                }
                            }
                        }
                        Ok(Message::Close(frame)) => {
                            let reason = frame.map_or_else(|| "Server closed connection".to_string(), |f| f.reason.to_string());
                            tracing::warn!("WebSocket closed: {reason}");
                            let _ = state_tx.send(ConnectionState::Disconnected {
                                reason: reason.clone(),
                            });
                            return reason;
                        }
                        Ok(Message::Ping(data)) => {
                            let _ = ws_sink.send(Message::Pong(data)).await;
                        }
                        Ok(Message::Pong(_)) => {
                            last_pong = tokio::time::Instant::now();
                            tracing::trace!("Received pong from server");
                        }
                        Ok(_) => {}
                        Err(e) => {
                            let reason = format!("WebSocket error: {e}");
                            let error_str = e.to_string();
                            if error_str.contains("Connection reset without closing handshake") {
                                tracing::debug!("{reason}");
                            } else {
                                tracing::error!("{reason}");
                            }
                            let _ = state_tx.send(ConnectionState::Disconnected {
                                reason: reason.clone(),
                            });
                            return reason;
                        }
                    }
                }

                _ = ping_timer.tick() => {
                    if config.pong_timeout_ms > 0 && last_pong.elapsed() > pong_timeout {
                        let reason = "Pong timeout".to_string();
                        let timeout_ms = config.pong_timeout_ms;
                        tracing::debug!("No pong received within {timeout_ms}ms, triggering reconnect");
                        let _ = state_tx.send(ConnectionState::Disconnected {
                            reason: reason.clone(),
                        });
                        return reason;
                    }

                    if let Err(e) = ws_sink.send(Message::Ping(vec![].into())).await {
                        let reason = format!("Failed to send ping: {e}");
                        tracing::warn!("{reason}");
                        let _ = state_tx.send(ConnectionState::Disconnected {
                            reason: reason.clone(),
                        });
                        return reason;
                    }
                    tracing::trace!("Sent keepalive ping");
                }

                else => {
                    return "Channel closed".to_string();
                }
            }
        }
    }

    /// Handle a message received from the server.
    async fn handle_server_message(
        msg: ServerMessage,
        pending_requests: &PendingRequestsMap,
        resumable_streams: &ResumableStreamsMap,
    ) {
        match msg {
            ServerMessage::Response(response) => {
                let mut pending = pending_requests.write().await;
                if let Some(tx) = pending.remove(&response.request_id) {
                    let _ = tx.send(Ok(response));
                }
            }
            ServerMessage::Error(error) => {
                let mut pending = pending_requests.write().await;
                if let Some(tx) = pending.remove(&error.request_id) {
                    let _ = tx.send(Err(error));
                }
            }
            ServerMessage::StreamData(data) => {
                let streams = resumable_streams.read().await;
                if let Some(stream) = streams.get(&data.id) {
                    let _ = stream.sender.send(data).await;
                }
            }
            ServerMessage::StreamEnd(end) => {
                let mut streams = resumable_streams.write().await;
                if let Some(stream) = streams.remove(&end.id) {
                    if let Some(ref on_end) = stream.on_end {
                        on_end();
                    }
                }
            }
            ServerMessage::Other(_) => {
                tracing::warn!("Received unknown server message type");
            }
        }
    }

    /// Cleanup pending requests on final shutdown.
    async fn cleanup_pending_requests(pending_requests: &PendingRequestsMap) {
        let mut pending_map = pending_requests.write().await;
        for (request_id, tx) in pending_map.drain() {
            let _ = tx.send(Err(ResponseError {
                request_id,
                code: 0,
                message: "Connection closed".to_string(),
            }));
        }
    }

    /// Drop all stream senders so `QuoteStream::recv()` returns `None` instead of hanging.
    async fn cleanup_resumable_streams(resumable_streams: &ResumableStreamsMap) {
        let mut streams = resumable_streams.write().await;
        for (_id, stream) in streams.drain() {
            if let Some(ref on_end) = stream.on_end {
                on_end();
            }
        }
    }

    /// Send a request and wait for response.
    #[tracing::instrument(skip_all)]
    pub async fn send_request(
        &self,
        data: RequestData,
    ) -> Result<ResponseSuccess, TitanClientError> {
        let request_id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = ClientRequest {
            id: request_id,
            data,
        };

        let (response_tx, response_rx) = oneshot::channel();

        self.sender
            .send(PendingRequest {
                request,
                response_tx,
            })
            .await
            .map_err(|_| TitanClientError::Unexpected(anyhow::anyhow!("Connection closed")))?;

        let response = response_rx.await.map_err(|_| {
            TitanClientError::Unexpected(anyhow::anyhow!("Response channel closed"))
        })?;

        response.map_err(|e| TitanClientError::ServerError {
            code: e.code,
            message: e.message,
        })
    }

    /// Register a resumable stream.
    pub async fn register_stream(
        &self,
        stream_id: u32,
        request: SwapQuoteRequest,
        sender: mpsc::Sender<StreamData>,
        on_end: Option<OnEndCallback>,
        effective_id: Option<Arc<AtomicU32>>,
    ) {
        let mut streams = self.resumable_streams.write().await;
        streams.insert(
            stream_id,
            ResumableStream {
                request,
                sender,
                on_end,
                effective_id,
            },
        );
    }

    /// Unregister a stream.
    pub async fn unregister_stream(&self, stream_id: u32) {
        let mut streams = self.resumable_streams.write().await;
        streams.remove(&stream_id);
    }

    /// Get a receiver for connection state changes.
    pub fn state_receiver(&self) -> tokio::sync::watch::Receiver<ConnectionState> {
        self.state_tx.subscribe()
    }

    /// Get the current connection state.
    pub fn state(&self) -> ConnectionState {
        self.state_tx.borrow().clone()
    }

    /// Get all active stream IDs.
    pub async fn active_stream_ids(&self) -> Vec<u32> {
        let streams = self.resumable_streams.read().await;
        streams.keys().copied().collect()
    }

    /// Stop all active streams gracefully.
    ///
    /// Sends StopStream for each active stream and clears the stream map.
    #[tracing::instrument(skip_all)]
    pub async fn stop_all_streams(&self) {
        use titan_api_types::ws::v1::StopStreamRequest;

        let stream_ids = self.active_stream_ids().await;

        if stream_ids.is_empty() {
            return;
        }

        tracing::info!("Stopping {} active streams", stream_ids.len());

        for stream_id in stream_ids {
            // Send stop request (fire and forget)
            let _ = self
                .send_request(RequestData::StopStream(StopStreamRequest { id: stream_id }))
                .await;
        }

        // Clear all streams and call on_end for each
        let mut streams = self.resumable_streams.write().await;
        for (_id, stream) in streams.drain() {
            if let Some(ref on_end) = stream.on_end {
                on_end();
            }
        }
    }

    /// Graceful shutdown: stop all streams and signal connection loop to exit.
    #[tracing::instrument(skip_all)]
    pub async fn shutdown(&self) {
        // Stop all streams first
        self.stop_all_streams().await;

        // Update state
        let _ = self.state_tx.send(ConnectionState::Disconnected {
            reason: "Client shutdown".to_string(),
        });

        // The connection loop will exit when it detects the sender is closed
        // (which happens when Connection is dropped)
    }
}

/// Calculate exponential backoff.
fn calculate_backoff(attempt: u32, max_delay_ms: u64) -> u64 {
    let base_delay = INITIAL_BACKOFF_MS * 2u64.saturating_pow(attempt.saturating_sub(1));
    base_delay.min(max_delay_ms)
}
