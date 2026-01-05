//! Main Titan client implementation.

use std::sync::Arc;

use titan_api_types::ws::v1::{
    GetInfoRequest, GetVenuesRequest, ListProvidersRequest, ProviderInfo, RequestData,
    ResponseData, ResponseSuccess, ServerInfo, SwapPrice, SwapPriceRequest, VenueInfo,
};
use tokio::sync::RwLock;

use crate::config::TitanConfig;
use crate::connection::Connection;
use crate::error::TitanClientError;
use crate::state::ConnectionState;

/// Titan Exchange WebSocket client.
///
/// Thread-safe client for interacting with the Titan Exchange API.
/// Can be shared across axum handlers via `Arc<TitanClient>`.
pub struct TitanClient {
    connection: Arc<RwLock<Option<Connection>>>,
    #[allow(dead_code)]
    config: TitanConfig,
}

impl TitanClient {
    /// Create a new client with the given configuration.
    ///
    /// Connects eagerly but doesn't fail if unreachable - retries in background.
    #[tracing::instrument(skip_all)]
    pub async fn new(config: TitanConfig) -> Result<Self, TitanClientError> {
        let connection = Connection::connect(config.clone()).await?;

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

    /// Send a request to the server and wait for response.
    #[tracing::instrument(skip_all)]
    async fn send_request(&self, data: RequestData) -> Result<ResponseSuccess, TitanClientError> {
        let conn = self.connection.read().await;
        if let Some(ref connection) = *conn {
            connection.send_request(data).await
        } else {
            Err(TitanClientError::ConnectionFailed {
                attempts: 0,
                reason: "Not connected".to_string(),
            })
        }
    }

    /// Get a reference to the underlying connection for stream registration.
    pub async fn connection(&self) -> Option<impl std::ops::Deref<Target = Connection> + '_> {
        let guard = self.connection.read().await;
        if guard.is_some() {
            Some(tokio::sync::RwLockReadGuard::map(guard, |opt| {
                opt.as_ref().unwrap()
            }))
        } else {
            None
        }
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
        let response = self
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
        let response = self
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
        let response = self
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
        let response = self
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
}
