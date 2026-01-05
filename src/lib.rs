//! Titan Exchange WebSocket API client for Rust.
//!
//! Provides real-time swap quote streaming and one-shot price queries.

pub mod client;
pub mod config;
pub mod connection;
pub mod error;
pub mod state;
pub mod stream;

// Re-export main types
pub use client::TitanClient;
pub use config::TitanConfig;
pub use error::TitanClientError;
pub use state::ConnectionState;

// Re-export titan-api-types for convenience
pub mod types {
    pub use titan_api_types::common::{AccountMeta, Instruction, Pubkey};
    pub use titan_api_types::ws::v1::{
        GetInfoRequest, GetVenuesRequest, ListProvidersRequest, PlatformFee, ProviderInfo,
        ProviderKind, QuoteSwapStreamResponse, QuoteUpdateParams, ResponseData, ResponseError,
        ResponseSuccess, RoutePlanStep, ServerInfo, ServerMessage, ServerSettings,
        StopStreamRequest, StopStreamResponse, StreamData, StreamDataPayload, StreamDataType,
        StreamEnd, StreamStart, SwapMode, SwapParams, SwapPrice, SwapPriceRequest,
        SwapQuoteRequest, SwapQuotes, SwapRoute, SwapSettings, TransactionParams,
        TransactionSettings, VenueInfo,
    };
}
