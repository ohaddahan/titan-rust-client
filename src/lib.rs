//! Titan Exchange WebSocket API client for Rust.
//!
//! Provides real-time swap quote streaming and one-shot price queries.

pub mod client;
pub mod config;
pub mod connection;
pub mod error;
#[cfg(feature = "solana")]
pub mod instructions;
pub mod queue;
pub mod state;
pub mod stream;
pub mod tls;

// Re-export main types
pub use client::TitanClient;
pub use config::TitanConfig;
pub use error::TitanClientError;
#[cfg(feature = "solana")]
pub use instructions::{TitanInstructions, TitanInstructionsOutput};
pub use queue::StreamManager;
pub use state::ConnectionState;
pub use stream::QuoteStream;

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
