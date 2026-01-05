//! Mock Titan WebSocket server for testing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use titan_api_types::common::{BoundedValueWithDefault, Pubkey};
use titan_api_types::ws::v1::{
    ClientRequest, ConnectionSettings, ProviderInfo, ProviderKind, QuoteSwapStreamResponse,
    QuoteUpdateSettings, RequestData, ResponseData, ResponseSuccess, ServerInfo, ServerMessage,
    ServerSettings, StopStreamResponse, StreamData, StreamDataPayload, StreamDataType, StreamStart,
    SwapMode, SwapPrice, SwapQuotes, SwapRoute, SwapSettings, TransactionSettings, VenueInfo,
    VersionInfo, WEBSOCKET_SUBPROTO_BASE,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;

/// Mock Titan WebSocket server for testing.
#[allow(dead_code)]
pub struct MockTitanServer {
    addr: SocketAddr,
    shutdown_tx: broadcast::Sender<()>,
    handle: JoinHandle<()>,
    config: Arc<RwLock<MockServerConfig>>,
    stream_counter: Arc<AtomicU32>,
    connected_clients: Arc<AtomicU32>,
}

/// Configuration for the mock server.
#[derive(Clone)]
#[allow(dead_code)]
pub struct MockServerConfig {
    pub server_info: ServerInfo,
    pub venues: VenueInfo,
    pub providers: Vec<ProviderInfo>,
    pub max_concurrent_streams: u32,
    pub reject_auth: bool,
    pub disconnect_after_requests: Option<u32>,
}

impl Default for MockServerConfig {
    fn default() -> Self {
        Self {
            server_info: default_server_info(),
            venues: default_venues(),
            providers: default_providers(),
            max_concurrent_streams: 10,
            reject_auth: false,
            disconnect_after_requests: None,
        }
    }
}

fn default_server_info() -> ServerInfo {
    ServerInfo {
        protocol_version: VersionInfo::new(1, 2, 0),
        settings: ServerSettings {
            quote_update: QuoteUpdateSettings {
                interval_ms: BoundedValueWithDefault {
                    min: 100,
                    max: 10000,
                    default: 500,
                },
                num_quotes: BoundedValueWithDefault {
                    min: 1,
                    max: 10,
                    default: 5,
                },
            },
            swap: SwapSettings {
                slippage_bps: BoundedValueWithDefault {
                    min: 0,
                    max: 10000,
                    default: 50,
                },
                only_direct_routes: false,
                add_size_constraint: true,
                size_constraint: 1232,
            },
            transaction: TransactionSettings {
                close_input_token_account: false,
                create_output_token_account: true,
            },
            connection: ConnectionSettings {
                concurrent_streams: 10,
            },
        },
    }
}

fn default_venues() -> VenueInfo {
    VenueInfo {
        labels: vec![
            "Raydium".to_string(),
            "Orca".to_string(),
            "Meteora".to_string(),
        ],
        program_ids: Some(vec![
            Pubkey::default(),
            Pubkey::default(),
            Pubkey::default(),
        ]),
    }
}

fn default_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "jupiter".to_string(),
            name: "Jupiter".to_string(),
            kind: ProviderKind::DexAggregator,
            icon_uri_48: None,
        },
        ProviderInfo {
            id: "titan".to_string(),
            name: "Titan".to_string(),
            kind: ProviderKind::DexAggregator,
            icon_uri_48: None,
        },
    ]
}

#[allow(dead_code)]
impl MockTitanServer {
    /// Start a mock server on a random available port.
    pub async fn start() -> Self {
        Self::start_with_config(MockServerConfig::default()).await
    }

    /// Start a mock server with custom configuration.
    pub async fn start_with_config(config: MockServerConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (shutdown_tx, _) = broadcast::channel(1);
        let config = Arc::new(RwLock::new(config));
        let stream_counter = Arc::new(AtomicU32::new(1));
        let connected_clients = Arc::new(AtomicU32::new(0));

        let handle = tokio::spawn(run_server(
            listener,
            shutdown_tx.subscribe(),
            config.clone(),
            stream_counter.clone(),
            connected_clients.clone(),
        ));

        Self {
            addr,
            shutdown_tx,
            handle,
            config,
            stream_counter,
            connected_clients,
        }
    }

    /// Get the WebSocket URL to connect to.
    pub fn url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    /// Get the number of connected clients.
    pub fn connected_clients(&self) -> u32 {
        self.connected_clients.load(Ordering::SeqCst)
    }

    /// Update server configuration.
    pub async fn set_config(&self, config: MockServerConfig) {
        *self.config.write().await = config;
    }

    /// Set max concurrent streams.
    pub async fn set_max_concurrent_streams(&self, max: u32) {
        let mut config = self.config.write().await;
        config.max_concurrent_streams = max;
        config.server_info.settings.connection.concurrent_streams = max;
    }

    /// Configure to reject authentication.
    pub async fn set_reject_auth(&self, reject: bool) {
        self.config.write().await.reject_auth = reject;
    }

    /// Configure to disconnect after N requests (for reconnect testing).
    pub async fn set_disconnect_after(&self, count: Option<u32>) {
        self.config.write().await.disconnect_after_requests = count;
    }

    /// Shutdown the server.
    pub async fn stop(self) {
        let _ = self.shutdown_tx.send(());
        let _ = self.handle.await;
    }
}

async fn run_server(
    listener: TcpListener,
    mut shutdown_rx: broadcast::Receiver<()>,
    config: Arc<RwLock<MockServerConfig>>,
    stream_counter: Arc<AtomicU32>,
    connected_clients: Arc<AtomicU32>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        connected_clients.fetch_add(1, Ordering::SeqCst);
                        let config = config.clone();
                        let stream_counter = stream_counter.clone();
                        let connected_clients = connected_clients.clone();
                        let shutdown_rx = shutdown_rx.resubscribe();

                        tokio::spawn(async move {
                            handle_connection(stream, shutdown_rx, config, stream_counter).await;
                            connected_clients.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) => {
                        eprintln!("Accept error: {}", e);
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }
}

async fn handle_connection(
    stream: TcpStream,
    mut shutdown_rx: broadcast::Receiver<()>,
    config: Arc<RwLock<MockServerConfig>>,
    stream_counter: Arc<AtomicU32>,
) {
    // Custom callback to respond with the subprotocol
    let callback = |req: &Request,
                    mut response: Response|
     -> Result<
        Response,
        tokio_tungstenite::tungstenite::http::Response<Option<String>>,
    > {
        // Check for Titan subprotocol in request headers and echo it back
        if let Some(protocols) = req.headers().get("Sec-WebSocket-Protocol") {
            if let Ok(protocols_str) = protocols.to_str() {
                if protocols_str.contains(WEBSOCKET_SUBPROTO_BASE) {
                    response.headers_mut().insert(
                        "Sec-WebSocket-Protocol",
                        WEBSOCKET_SUBPROTO_BASE.parse().unwrap(),
                    );
                }
            }
        }
        Ok(response)
    };

    let ws_stream = match tokio_tungstenite::accept_hdr_async(stream, callback).await {
        Ok(ws) => ws,
        Err(_) => return,
    };

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let request_count = Arc::new(AtomicU32::new(0));
    let active_streams: Arc<RwLock<HashMap<u32, mpsc::Sender<()>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let should_disconnect = Arc::new(AtomicBool::new(false));

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        let cfg = config.read().await.clone();

                        // Check if we should disconnect
                        if let Some(max) = cfg.disconnect_after_requests {
                            let count = request_count.fetch_add(1, Ordering::SeqCst) + 1;
                            if count >= max {
                                should_disconnect.store(true, Ordering::SeqCst);
                            }
                        }

                        if should_disconnect.load(Ordering::SeqCst) {
                            break;
                        }

                        // Decode the request
                        let request: ClientRequest = match rmp_serde::from_slice::<ClientRequest>(&data) {
                            Ok(req) => req,
                            Err(_) => continue,
                        };

                        // Handle the request
                        let response = handle_request(
                            request,
                            &cfg,
                            &stream_counter,
                            &active_streams,
                        ).await;

                        // Encode and send response
                        if let Some(msg) = response {
                            let encoded = rmp_serde::to_vec_named(&msg).unwrap();
                            if ws_tx.send(Message::Binary(encoded.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    // Stop all active streams
    let streams = active_streams.read().await;
    for sender in streams.values() {
        let _ = sender.send(()).await;
    }
}

async fn handle_request(
    request: ClientRequest,
    config: &MockServerConfig,
    stream_counter: &Arc<AtomicU32>,
    active_streams: &Arc<RwLock<HashMap<u32, mpsc::Sender<()>>>>,
) -> Option<ServerMessage> {
    let request_id = request.id;

    match request.data {
        RequestData::GetInfo(_) => Some(ServerMessage::Response(ResponseSuccess {
            request_id,
            data: ResponseData::GetInfo(config.server_info.clone()),
            stream: None,
        })),
        RequestData::GetVenues(_) => Some(ServerMessage::Response(ResponseSuccess {
            request_id,
            data: ResponseData::GetVenues(config.venues.clone()),
            stream: None,
        })),
        RequestData::ListProviders(_) => Some(ServerMessage::Response(ResponseSuccess {
            request_id,
            data: ResponseData::ListProviders(config.providers.clone()),
            stream: None,
        })),
        RequestData::GetSwapPrice(req) => {
            let price = SwapPrice {
                id: format!("price-{}", request_id),
                input_mint: req.input_mint,
                output_mint: req.output_mint,
                amount_in: req.amount,
                amount_out: req.amount * 150, // Mock 150x price
            };
            Some(ServerMessage::Response(ResponseSuccess {
                request_id,
                data: ResponseData::GetSwapPrice(price),
                stream: None,
            }))
        }
        RequestData::NewSwapQuoteStream(_req) => {
            let stream_id = stream_counter.fetch_add(1, Ordering::SeqCst);

            // Store stream for later cancellation
            let (stop_tx, _stop_rx) = mpsc::channel(1);
            active_streams.write().await.insert(stream_id, stop_tx);

            Some(ServerMessage::Response(ResponseSuccess {
                request_id,
                data: ResponseData::NewSwapQuoteStream(QuoteSwapStreamResponse {
                    interval_ms: 500,
                    num_quotes: 5,
                }),
                stream: Some(StreamStart {
                    id: stream_id,
                    data_type: StreamDataType::SwapQuotes,
                }),
            }))
        }
        RequestData::StopStream(req) => {
            let stream_id = req.id;
            active_streams.write().await.remove(&stream_id);
            // Return Response (not StreamEnd) so client's send_request gets resolved
            Some(ServerMessage::Response(ResponseSuccess {
                request_id,
                data: ResponseData::StreamStopped(StopStreamResponse { id: stream_id }),
                stream: None,
            }))
        }
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

/// Generate mock SwapQuotes for testing.
#[allow(dead_code)]
pub fn mock_swap_quotes(
    input_mint: Pubkey,
    output_mint: Pubkey,
    in_amount: u64,
    out_amount: u64,
) -> SwapQuotes {
    let mut quotes = HashMap::new();
    quotes.insert(
        "mock-provider".to_string(),
        SwapRoute {
            in_amount,
            out_amount,
            slippage_bps: 50,
            platform_fee: None,
            steps: vec![],
            instructions: vec![],
            address_lookup_tables: vec![],
            context_slot: Some(12345678),
            time_taken_ns: Some(1000000),
            expires_at_ms: None,
            expires_after_slot: None,
            compute_units: Some(200000),
            compute_units_safe: Some(300000),
            transaction: None,
            reference_id: None,
        },
    );

    SwapQuotes {
        id: "quote-1".to_string(),
        input_mint,
        output_mint,
        swap_mode: SwapMode::ExactIn,
        amount: in_amount,
        quotes,
    }
}

/// Create mock stream data message.
#[allow(dead_code)]
pub fn mock_stream_data(stream_id: u32, seq: u32, quotes: SwapQuotes) -> ServerMessage {
    ServerMessage::StreamData(StreamData {
        id: stream_id,
        seq,
        payload: StreamDataPayload::SwapQuotes(quotes),
    })
}
