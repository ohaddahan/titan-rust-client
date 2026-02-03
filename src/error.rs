//! Error types for the Titan client.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TitanClientError {
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("Stream limit exceeded")]
    StreamLimitExceeded,

    #[error("Connection failed after {attempts} attempts: {reason}")]
    ConnectionFailed { attempts: u32, reason: String },

    #[error("Connection closed: {reason}")]
    ConnectionClosed { reason: String },

    #[error("Server error {code}: {message}")]
    ServerError { code: u32, message: String },

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] rmp_serde::encode::Error),

    #[error("Deserialization error: {0}")]
    Deserialization(#[from] rmp_serde::decode::Error),

    #[error("Operation timed out after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}
