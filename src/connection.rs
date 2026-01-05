//! WebSocket connection management.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use titan_api_codec::codec::ws::v1::ClientCodec;
use titan_api_codec::codec::Codec;
use titan_api_types::ws::v1::{
    ClientRequest, RequestData, ResponseError, ResponseSuccess, ServerMessage, StreamData,
};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::config::TitanConfig;
use crate::error::TitanClientError;
use crate::state::ConnectionState;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type ResponseResult = Result<ResponseSuccess, ResponseError>;
type PendingRequestsMap = Arc<RwLock<HashMap<u32, oneshot::Sender<ResponseResult>>>>;
type StreamSendersMap = Arc<RwLock<HashMap<u32, mpsc::Sender<StreamData>>>>;

/// Internal message for sending requests through the connection
pub struct PendingRequest {
    pub request: ClientRequest,
    pub response_tx: oneshot::Sender<ResponseResult>,
}

/// Manages a WebSocket connection to the Titan API
pub struct Connection {
    #[allow(dead_code)]
    config: TitanConfig,
    request_id: AtomicU32,
    sender: mpsc::Sender<PendingRequest>,
    state_tx: tokio::sync::watch::Sender<ConnectionState>,
    #[allow(dead_code)]
    pending_requests: PendingRequestsMap,
    stream_senders: StreamSendersMap,
}

impl Connection {
    /// Create a new connection with the given config
    #[tracing::instrument(skip_all)]
    pub async fn connect(config: TitanConfig) -> Result<Self, TitanClientError> {
        let (state_tx, _state_rx) = tokio::sync::watch::channel(ConnectionState::Disconnected {
            reason: "Connecting...".to_string(),
        });

        let pending_requests: PendingRequestsMap = Arc::new(RwLock::new(HashMap::new()));
        let stream_senders: StreamSendersMap = Arc::new(RwLock::new(HashMap::new()));

        // Connect to WebSocket
        let ws_stream = Self::establish_connection(&config).await?;

        // Create channel for sending requests
        let (sender, receiver) = mpsc::channel::<PendingRequest>(32);

        // Spawn background task
        let pending_clone = pending_requests.clone();
        let streams_clone = stream_senders.clone();
        let state_tx_clone = state_tx.clone();

        tokio::spawn(Self::run_connection_loop(
            ws_stream,
            receiver,
            pending_clone,
            streams_clone,
            state_tx_clone,
        ));

        state_tx.send_replace(ConnectionState::Connected);

        Ok(Self {
            config,
            request_id: AtomicU32::new(1),
            sender,
            state_tx,
            pending_requests,
            stream_senders,
        })
    }

    /// Establish WebSocket connection with authentication
    async fn establish_connection(config: &TitanConfig) -> Result<WsStream, TitanClientError> {
        // Build URL with auth query parameter
        let url = format!("{}?auth={}", config.url, config.token);
        let uri: Uri = url
            .parse()
            .map_err(|e| TitanClientError::Unexpected(anyhow::anyhow!("Invalid URL: {}", e)))?;

        // Connect with subprotocol header for uncompressed MessagePack
        let request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(uri)
            .header(
                "Sec-WebSocket-Protocol",
                titan_api_types::ws::v1::WEBSOCKET_SUBPROTO_BASE,
            )
            .header("Host", extract_host(&config.url).unwrap_or("api.titan.ag"))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| {
                TitanClientError::Unexpected(anyhow::anyhow!("Failed to build request: {}", e))
            })?;

        let (ws_stream, _response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(TitanClientError::WebSocket)?;

        Ok(ws_stream)
    }

    /// Background task that handles WebSocket I/O
    async fn run_connection_loop(
        ws_stream: WsStream,
        mut request_rx: mpsc::Receiver<PendingRequest>,
        pending_requests: PendingRequestsMap,
        stream_senders: StreamSendersMap,
        state_tx: tokio::sync::watch::Sender<ConnectionState>,
    ) {
        let codec = ClientCodec::Uncompressed;
        let mut encoder = codec.encoder();
        let mut decoder = codec.decoder();

        let (mut ws_sink, mut ws_stream) = ws_stream.split();

        loop {
            tokio::select! {
                // Handle outgoing requests
                Some(pending_req) = request_rx.recv() => {
                    let request_id = pending_req.request.id;

                    // Store the response channel
                    {
                        let mut pending_map = pending_requests.write().await;
                        pending_map.insert(request_id, pending_req.response_tx);
                    }

                    // Encode and send
                    match encoder.encode_mut(&pending_req.request) {
                        Ok(data) => {
                            if let Err(e) = ws_sink.send(Message::Binary(data.to_vec().into())).await {
                                tracing::error!("Failed to send WebSocket message: {}", e);
                                // Remove pending request and notify of error
                                let mut pending_map = pending_requests.write().await;
                                if let Some(tx) = pending_map.remove(&request_id) {
                                    let _ = tx.send(Err(ResponseError {
                                        request_id,
                                        code: 0,
                                        message: format!("Send failed: {}", e),
                                    }));
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to encode request: {}", e);
                            let mut pending_map = pending_requests.write().await;
                            if let Some(tx) = pending_map.remove(&request_id) {
                                let _ = tx.send(Err(ResponseError {
                                    request_id,
                                    code: 0,
                                    message: format!("Encode failed: {}", e),
                                }));
                            }
                        }
                    }
                }

                // Handle incoming messages
                Some(msg_result) = ws_stream.next() => {
                    match msg_result {
                        Ok(Message::Binary(data)) => {
                            match decoder.decode_mut(data) {
                                Ok(server_msg) => {
                                    Self::handle_server_message(
                                        server_msg,
                                        &pending_requests,
                                        &stream_senders,
                                    ).await;
                                }
                                Err(e) => {
                                    tracing::error!("Failed to decode server message: {}", e);
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            tracing::warn!("WebSocket connection closed by server");
                            let _ = state_tx.send(ConnectionState::Disconnected {
                                reason: "Server closed connection".to_string(),
                            });
                            break;
                        }
                        Ok(Message::Ping(data)) => {
                            let _ = ws_sink.send(Message::Pong(data)).await;
                        }
                        Ok(_) => {
                            // Ignore text and other message types
                        }
                        Err(e) => {
                            tracing::error!("WebSocket error: {}", e);
                            let _ = state_tx.send(ConnectionState::Disconnected {
                                reason: format!("WebSocket error: {}", e),
                            });
                            break;
                        }
                    }
                }

                else => break,
            }
        }

        // Cleanup: notify all pending requests of disconnection
        let mut pending_map = pending_requests.write().await;
        for (request_id, tx) in pending_map.drain() {
            let _ = tx.send(Err(ResponseError {
                request_id,
                code: 0,
                message: "Connection closed".to_string(),
            }));
        }
    }

    /// Handle a message received from the server
    async fn handle_server_message(
        msg: ServerMessage,
        pending_requests: &PendingRequestsMap,
        stream_senders: &StreamSendersMap,
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
                let senders = stream_senders.read().await;
                if let Some(tx) = senders.get(&data.id) {
                    let _ = tx.send(data).await;
                }
            }
            ServerMessage::StreamEnd(end) => {
                let mut senders = stream_senders.write().await;
                senders.remove(&end.id);
                // Stream ended, sender dropped -> receiver will see channel closed
            }
            ServerMessage::Other(_) => {
                tracing::warn!("Received unknown server message type");
            }
        }
    }

    /// Send a request and wait for response
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

    /// Register a stream sender for receiving stream data
    pub async fn register_stream(&self, stream_id: u32, sender: mpsc::Sender<StreamData>) {
        let mut senders = self.stream_senders.write().await;
        senders.insert(stream_id, sender);
    }

    /// Unregister a stream
    pub async fn unregister_stream(&self, stream_id: u32) {
        let mut senders = self.stream_senders.write().await;
        senders.remove(&stream_id);
    }

    /// Get a receiver for connection state changes
    pub fn state_receiver(&self) -> tokio::sync::watch::Receiver<ConnectionState> {
        self.state_tx.subscribe()
    }
}

/// Extract host from URL for the Host header
fn extract_host(url: &str) -> Option<&str> {
    url.strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.split('?').next())
}
