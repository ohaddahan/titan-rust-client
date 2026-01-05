//! Main Titan client implementation.

use std::sync::Arc;

use titan_api_types::ws::v1::{
    GetInfoRequest, GetVenuesRequest, ListProvidersRequest, ProviderInfo, RequestData,
    ResponseData, ServerInfo, StreamDataPayload, SwapPrice, SwapPriceRequest, SwapQuoteRequest,
    SwapQuotes, VenueInfo,
};
use tokio::sync::{mpsc, RwLock};

use crate::config::TitanConfig;
use crate::connection::Connection;
use crate::error::TitanClientError;
use crate::state::ConnectionState;
use crate::stream::QuoteStream;

/// Titan Exchange WebSocket client.
///
/// Thread-safe client for interacting with the Titan Exchange API.
/// Can be shared across axum handlers via `Arc<TitanClient>`.
pub struct TitanClient {
    connection: Arc<RwLock<Option<Arc<Connection>>>>,
    #[allow(dead_code)]
    config: TitanConfig,
}

impl TitanClient {
    /// Create a new client with the given configuration.
    ///
    /// Connects eagerly but doesn't fail if unreachable - retries in background.
    #[tracing::instrument(skip_all)]
    pub async fn new(config: TitanConfig) -> Result<Self, TitanClientError> {
        let connection = Arc::new(Connection::connect(config.clone()).await?);

        Ok(Self {
            connection: Arc::new(RwLock::new(Some(connection))),
            config,
        })
    }

    /// Returns a watch receiver for connection state changes.
    pub async fn connection_state(&self) -> tokio::sync::watch::Receiver<ConnectionState> {
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

    /// Get a clone of the connection Arc.
    async fn get_connection(&self) -> Result<Arc<Connection>, TitanClientError> {
        let conn = self.connection.read().await;
        conn.clone()
            .ok_or_else(|| TitanClientError::ConnectionFailed {
                attempts: 0,
                reason: "Not connected".to_string(),
            })
    }

    /// Graceful shutdown: stops all streams, then closes WebSocket.
    #[tracing::instrument(skip_all)]
    pub async fn close(&self) -> Result<(), TitanClientError> {
        let mut conn = self.connection.write().await;
        *conn = None;
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
    #[tracing::instrument(skip_all)]
    pub async fn new_swap_quote_stream(
        &self,
        request: SwapQuoteRequest,
    ) -> Result<QuoteStream, TitanClientError> {
        let connection = self.get_connection().await?;

        // Send the stream request
        let response = connection
            .send_request(RequestData::NewSwapQuoteStream(request))
            .await?;

        // Extract stream ID from response
        let stream_id = response
            .stream
            .ok_or_else(|| {
                TitanClientError::Unexpected(anyhow::anyhow!(
                    "NewSwapQuoteStream response missing stream info"
                ))
            })?
            .id;

        // Create channels for stream data
        // Raw StreamData channel (from connection)
        let (raw_tx, mut raw_rx) = mpsc::channel::<titan_api_types::ws::v1::StreamData>(32);
        // Processed SwapQuotes channel (to user)
        let (quotes_tx, quotes_rx) = mpsc::channel::<SwapQuotes>(32);

        // Register the raw stream with the connection
        connection.register_stream(stream_id, raw_tx).await;

        // Spawn adapter task to convert StreamData -> SwapQuotes
        let adapter_connection = connection.clone();
        tokio::spawn(async move {
            while let Some(data) = raw_rx.recv().await {
                match data.payload {
                    StreamDataPayload::SwapQuotes(quotes) => {
                        if quotes_tx.send(quotes).await.is_err() {
                            // Receiver dropped, unregister and stop
                            adapter_connection.unregister_stream(stream_id).await;
                            break;
                        }
                    }
                    #[allow(unreachable_patterns)]
                    _ => {
                        tracing::warn!("Received unexpected stream data payload type");
                    }
                }
            }
        });

        Ok(QuoteStream::new(stream_id, quotes_rx, connection))
    }
}
