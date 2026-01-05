//! Main Titan client implementation.

use crate::config::TitanConfig;
use crate::error::TitanClientError;
use crate::state::ConnectionState;

/// Titan Exchange WebSocket client.
///
/// Thread-safe client for interacting with the Titan Exchange API.
/// Can be shared across axum handlers via `Arc<TitanClient>`.
#[derive(Debug)]
pub struct TitanClient {
    // TODO for now, need to remove later
    #[allow(dead_code)]
    config: TitanConfig,
}

impl TitanClient {
    /// Create a new client with the given configuration.
    ///
    /// Connects eagerly but doesn't fail if unreachable - retries in background.
    #[tracing::instrument(skip_all)]
    pub async fn new(config: TitanConfig) -> Result<Self, TitanClientError> {
        // TODO: Implement connection in step 3
        Ok(Self { config })
    }

    /// Returns a watch receiver for connection state changes.
    pub fn connection_state(&self) -> tokio::sync::watch::Receiver<ConnectionState> {
        // TODO: Implement in step 10
        let (tx, rx) = tokio::sync::watch::channel(ConnectionState::default());
        drop(tx);
        rx
    }

    /// Graceful shutdown: stops all streams, then closes WebSocket.
    #[tracing::instrument(skip_all)]
    pub async fn close(&self) -> Result<(), TitanClientError> {
        // TODO: Implement in step 11
        Ok(())
    }
}
