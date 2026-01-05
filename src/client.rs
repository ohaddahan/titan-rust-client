//! Main Titan client implementation.

use std::sync::Arc;

use titan_api_types::ws::v1::{RequestData, ResponseSuccess};
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
    pub async fn send_request(
        &self,
        data: RequestData,
    ) -> Result<ResponseSuccess, TitanClientError> {
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
}
