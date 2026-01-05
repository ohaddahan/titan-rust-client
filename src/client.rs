//! Main Titan client implementation.

use std::sync::Arc;

use titan_api_types::ws::v1::{
    GetInfoRequest, GetVenuesRequest, ListProvidersRequest, ProviderInfo, RequestData,
    ResponseData, ServerInfo, SwapPrice, SwapPriceRequest, SwapQuoteRequest, VenueInfo,
};
use tokio::sync::RwLock;

use crate::config::TitanConfig;
use crate::connection::Connection;
use crate::error::TitanClientError;
use crate::queue::StreamManager;
use crate::state::ConnectionState;
use crate::stream::QuoteStream;

/// Default max concurrent streams if server doesn't specify.
const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 10;

/// Titan Exchange WebSocket client.
///
/// Thread-safe client for interacting with the Titan Exchange API.
/// Can be shared across axum handlers via `Arc<TitanClient>`.
pub struct TitanClient {
    connection: Arc<RwLock<Option<Arc<Connection>>>>,
    stream_manager: Arc<RwLock<Option<Arc<StreamManager>>>>,
    #[allow(dead_code)]
    config: TitanConfig,
}

impl TitanClient {
    /// Create a new client with the given configuration.
    ///
    /// Connects eagerly and fetches server info to determine stream limits.
    #[tracing::instrument(skip_all)]
    pub async fn new(config: TitanConfig) -> Result<Self, TitanClientError> {
        let connection = Arc::new(Connection::connect(config.clone()).await?);

        // Fetch server info to get max concurrent streams
        let max_streams = Self::fetch_max_streams(&connection).await;

        let stream_manager = StreamManager::new(connection.clone(), max_streams);

        Ok(Self {
            connection: Arc::new(RwLock::new(Some(connection))),
            stream_manager: Arc::new(RwLock::new(Some(stream_manager))),
            config,
        })
    }

    /// Fetch max concurrent streams from server info.
    async fn fetch_max_streams(connection: &Arc<Connection>) -> u32 {
        match connection
            .send_request(RequestData::GetInfo(GetInfoRequest { dummy: None }))
            .await
        {
            Ok(response) => match response.data {
                ResponseData::GetInfo(info) => info.settings.connection.concurrent_streams,
                _ => DEFAULT_MAX_CONCURRENT_STREAMS,
            },
            Err(e) => {
                tracing::warn!("Failed to fetch server info: {}, using default limits", e);
                DEFAULT_MAX_CONCURRENT_STREAMS
            }
        }
    }

    /// Returns a watch receiver for connection state changes.
    ///
    /// Use this to observe state transitions (Connected, Reconnecting, Disconnected).
    pub async fn state_receiver(&self) -> tokio::sync::watch::Receiver<ConnectionState> {
        let conn = self.connection.read().await;
        if let Some(ref connection) = *conn {
            connection.state_receiver()
        } else {
            let (tx, rx) = tokio::sync::watch::channel(ConnectionState::Disconnected {
                reason: "Not connected".to_string(),
            });
            drop(tx);
            rx
        }
    }

    /// Get the current connection state.
    pub async fn state(&self) -> ConnectionState {
        let conn = self.connection.read().await;
        if let Some(ref connection) = *conn {
            connection.state()
        } else {
            ConnectionState::Disconnected {
                reason: "Not connected".to_string(),
            }
        }
    }

    /// Returns true if currently connected.
    pub async fn is_connected(&self) -> bool {
        self.state().await.is_connected()
    }

    /// Wait until the connection is established.
    ///
    /// Returns immediately if already connected.
    /// Returns error if connection is permanently closed.
    pub async fn wait_for_connected(&self) -> Result<(), TitanClientError> {
        let mut receiver = self.state_receiver().await;

        loop {
            let state = receiver.borrow_and_update().clone();
            match state {
                ConnectionState::Connected => return Ok(()),
                ConnectionState::Disconnected { reason } => {
                    // Check if this is a permanent disconnection
                    if reason.contains("Max reconnect attempts") {
                        return Err(TitanClientError::ConnectionFailed {
                            attempts: 0,
                            reason,
                        });
                    }
                }
                ConnectionState::Reconnecting { .. } => {}
            }

            // Wait for next state change
            if receiver.changed().await.is_err() {
                return Err(TitanClientError::ConnectionFailed {
                    attempts: 0,
                    reason: "Connection closed".to_string(),
                });
            }
        }
    }

    /// Get a clone of the connection Arc.
    async fn get_connection(&self) -> Result<Arc<Connection>, TitanClientError> {
        let conn = self.connection.read().await;
        conn.clone()
            .ok_or_else(|| TitanClientError::ConnectionFailed {
                attempts: 0,
                reason: "Not connected".to_string(),
            })
    }

    /// Get a clone of the stream manager Arc.
    async fn get_stream_manager(&self) -> Result<Arc<StreamManager>, TitanClientError> {
        let manager = self.stream_manager.read().await;
        manager
            .clone()
            .ok_or_else(|| TitanClientError::ConnectionFailed {
                attempts: 0,
                reason: "Not connected".to_string(),
            })
    }

    /// Get the current active stream count.
    pub async fn active_stream_count(&self) -> u32 {
        match self.get_stream_manager().await {
            Ok(manager) => manager.active_count(),
            Err(_) => 0,
        }
    }

    /// Get the current queue length.
    pub async fn queued_stream_count(&self) -> usize {
        match self.get_stream_manager().await {
            Ok(manager) => manager.queue_len().await,
            Err(_) => 0,
        }
    }

    /// Graceful shutdown: stops all streams, then closes WebSocket.
    #[tracing::instrument(skip_all)]
    pub async fn close(&self) -> Result<(), TitanClientError> {
        {
            let mut manager = self.stream_manager.write().await;
            *manager = None;
        }
        {
            let mut conn = self.connection.write().await;
            *conn = None;
        }
        Ok(())
    }

    // ========== One-shot API methods ==========

    /// Get server info and connection limits.
    #[tracing::instrument(skip_all)]
    pub async fn get_info(&self) -> Result<ServerInfo, TitanClientError> {
        let connection = self.get_connection().await?;
        let response = connection
            .send_request(RequestData::GetInfo(GetInfoRequest { dummy: None }))
            .await?;

        match response.data {
            ResponseData::GetInfo(info) => Ok(info),
            other => Err(TitanClientError::Unexpected(anyhow::anyhow!(
                "Unexpected response type: expected GetInfo, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    /// Get available venues.
    #[tracing::instrument(skip_all)]
    pub async fn get_venues(&self) -> Result<VenueInfo, TitanClientError> {
        let connection = self.get_connection().await?;
        let response = connection
            .send_request(RequestData::GetVenues(GetVenuesRequest {
                include_program_ids: Some(true),
            }))
            .await?;

        match response.data {
            ResponseData::GetVenues(venues) => Ok(venues),
            other => Err(TitanClientError::Unexpected(anyhow::anyhow!(
                "Unexpected response type: expected GetVenues, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    /// List available providers.
    #[tracing::instrument(skip_all)]
    pub async fn list_providers(&self) -> Result<Vec<ProviderInfo>, TitanClientError> {
        let connection = self.get_connection().await?;
        let response = connection
            .send_request(RequestData::ListProviders(ListProvidersRequest {
                include_icons: Some(true),
            }))
            .await?;

        match response.data {
            ResponseData::ListProviders(providers) => Ok(providers),
            other => Err(TitanClientError::Unexpected(anyhow::anyhow!(
                "Unexpected response type: expected ListProviders, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    /// Get a point-in-time swap price.
    #[tracing::instrument(skip_all)]
    pub async fn get_swap_price(
        &self,
        request: SwapPriceRequest,
    ) -> Result<SwapPrice, TitanClientError> {
        let connection = self.get_connection().await?;
        let response = connection
            .send_request(RequestData::GetSwapPrice(request))
            .await?;

        match response.data {
            ResponseData::GetSwapPrice(price) => Ok(price),
            other => Err(TitanClientError::Unexpected(anyhow::anyhow!(
                "Unexpected response type: expected GetSwapPrice, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    // ========== Streaming API methods ==========

    /// Start a new swap quote stream.
    ///
    /// Returns a `QuoteStream` that yields `SwapQuotes` updates.
    /// The stream automatically sends `StopStream` when dropped.
    ///
    /// If the server's max concurrent streams limit is reached, the request
    /// will be queued and started automatically when a slot becomes available.
    #[tracing::instrument(skip_all)]
    pub async fn new_swap_quote_stream(
        &self,
        request: SwapQuoteRequest,
    ) -> Result<QuoteStream, TitanClientError> {
        let manager = self.get_stream_manager().await?;
        manager.request_stream(request).await
    }
}
