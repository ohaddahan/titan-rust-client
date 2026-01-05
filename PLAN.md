# Titan Rust Client Specification

## Overview

A Rust client library for the Titan Exchange API, designed as both a publishable lib crate and a local test binary. The client will be integrated into `/Users/ohaddahan/RustroverProjects/zklsol/worker-service` as a shared singleton.

## Crate Structure

```
titan-rust-client/
├── Cargo.toml
├── src/
│   ├── lib.rs                 # Library entry point
│   ├── client.rs              # TitanClient implementation
│   ├── config.rs              # TitanConfig struct
│   ├── connection.rs          # WebSocket connection management
│   ├── error.rs               # TitanClientError enum
│   ├── state.rs               # Connection state observable
│   ├── stream.rs              # Stream management and queue
│   ├── types.rs               # Local type definitions (if titan-api-codec lacks any)
│   └── backoff.rs             # Exponential backoff logic
└── src/bin/
    └── titan-cli.rs           # CLI test binary
```

**Crate name:** `titan-rust-client`

## Dependencies

### Required
- `titan-api-codec` - MessagePack serialization types (re-exported directly)
- `titan-api-types` - API type definitions (re-exported directly)
- `tokio` - Async runtime (hard dependency)
- `tokio-tungstenite` - WebSocket client
- `rmp-serde` - MessagePack serialization
- `thiserror` - Error type derivation
- `anyhow` - Error handling for unexpected failures
- `tracing` - Observability via `#[instrument]` spans
- `solana-sdk` - For instruction/account types in output

### CLI Binary
- `clap` - Command-line argument parsing
- `dotenvy` - Environment variable loading

## Configuration

```rust
pub struct TitanConfig {
    /// WebSocket URL (e.g., wss://api.titan.ag/api/v1/ws)
    pub url: String,

    /// JWT token for authentication
    pub token: String,

    /// Maximum reconnection delay in milliseconds (default: 30000)
    pub max_reconnect_delay_ms: u64,

    /// Maximum reconnection attempts before giving up (default: unlimited/None)
    pub max_reconnect_attempts: Option<u32>,

    /// Whether to wrap/unwrap SOL automatically (default: false)
    pub auto_wrap_sol: bool,
}
```

## Client API

### Initialization

```rust
impl TitanClient {
    /// Create client with config. Connects eagerly but doesn't fail if
    /// unreachable - retries in background.
    pub async fn new(config: TitanConfig) -> Result<Self, TitanClientError>;

    /// Graceful shutdown: stops all streams, then closes WebSocket
    pub async fn close(&self) -> Result<(), TitanClientError>;
}
```

### Connection State Observable

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connected,
    Reconnecting { attempt: u32 },
    Disconnected { reason: String },
}

impl TitanClient {
    /// Returns a watch receiver for connection state changes
    pub fn connection_state(&self) -> tokio::sync::watch::Receiver<ConnectionState>;
}
```

### API Methods

All methods from the Titan API:

```rust
impl TitanClient {
    // === One-shot queries ===

    /// Get server info and connection limits
    pub async fn get_info(&self) -> Result<ServerInfo, TitanClientError>;

    /// Get available venues
    pub async fn get_venues(&self) -> Result<VenueInfo, TitanClientError>;

    /// List providers
    pub async fn list_providers(&self) -> Result<Vec<ProviderInfo>, TitanClientError>;

    /// Get point-in-time swap price
    pub async fn get_swap_price(&self, request: SwapPriceRequest) -> Result<SwapPrice, TitanClientError>;

    // === Streaming ===

    /// Start a new swap quote stream. Returns channel receiver for quotes.
    /// When receiver is dropped, StopStream is sent automatically.
    pub async fn new_swap_quote_stream(
        &self,
        request: SwapQuoteRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<SwapQuote>, TitanClientError>;

    /// Explicitly stop a stream by ID
    pub async fn stop_stream(&self, stream_id: StreamId) -> Result<(), TitanClientError>;
}
```

### Instruction Output

**Key finding from research:** Titan embeds transaction data directly in the quote response. No separate endpoint needed.

The `SwapRoute` (part of `SwapQuotes`) contains two mutually exclusive paths:

1. **Individual instructions** (preferred for custom transaction building):
   - `instructions: Vec<Instruction>` - program_id, accounts, data
   - `address_lookup_tables: Vec<Pubkey>` - ALT pubkeys (must fetch from chain)

2. **Pre-built transaction** (alternative):
   - `transaction: Option<Vec<u8>>` - serialized `VersionedTransaction` ready to sign

The `SwapQuoteRequest` includes `TransactionParams` with `user_public_key`, so instructions are generated at quote time.

```rust
/// Helper to extract instruction data from a SwapRoute for transaction building
pub struct TitanInstructionsOutput {
    /// Address lookup table accounts (fetched from chain using ALT pubkeys)
    pub address_lookup_table_accounts: Vec<AddressLookupTableAccount>,

    /// Instructions to include in the transaction
    pub instructions: Vec<solana_instruction::Instruction>,

    /// Recommended compute units for the transaction
    pub compute_units_safe: Option<u64>,
}

impl TitanClient {
    /// Fetch address lookup tables and convert SwapRoute to transaction-ready output.
    /// Call this after selecting a route from SwapQuotes.
    pub async fn prepare_instructions(
        &self,
        route: &SwapRoute,
        rpc_client: &RpcClient,
    ) -> Result<TitanInstructionsOutput, TitanClientError>;
}
```

**Usage pattern** (similar to Jupiter):
```rust
// 1. Request quotes with user pubkey
let request = SwapQuoteRequest {
    swap: SwapParams { input_mint, output_mint, amount, .. },
    transaction: TransactionParams { user_public_key, .. },
    ..
};

// 2. Stream quotes
let mut rx = client.new_swap_quote_stream(request).await?;
while let Some(quotes) = rx.recv().await {
    // 3. Select best route
    let best_route = quotes.quotes.values().max_by_key(|r| r.out_amount)?;

    // 4. Either use pre-built transaction OR build your own
    if let Some(tx_bytes) = &best_route.transaction {
        // Option A: Sign pre-built transaction
        let tx = VersionedTransaction::deserialize(tx_bytes)?;
        tx.sign(&[&keypair]);
    } else {
        // Option B: Build custom transaction from instructions
        let output = client.prepare_instructions(best_route, &rpc_client).await?;
        // Use output.instructions, output.address_lookup_table_accounts
        // to build your own transaction
    }
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum TitanClientError {
    /// Authentication failed - caller should reconnect with new token
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Rate limited by server
    #[error("Rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    /// Stream limit exceeded (should not happen with internal queue, but defensive)
    #[error("Stream limit exceeded")]
    StreamLimitExceeded,

    /// Connection lost and reconnection failed
    #[error("Connection failed after {attempts} attempts: {reason}")]
    ConnectionFailed { attempts: u32, reason: String },

    /// Server returned an error
    #[error("Server error {code}: {message}")]
    ServerError { code: i32, message: String },

    /// Unexpected error (wraps anyhow)
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}
```

## Connection Management

### Reconnection Behavior

- **Auto-reconnect:** On WebSocket disconnect, automatically reconnect with exponential backoff
- **Stream resumption:** After reconnect, silently re-issue all active `newSwapQuoteStream` requests with original parameters. Callers see uninterrupted channel delivery.
- **Backoff:** 1s → 2s → 4s → 8s... with jitter, capped at `max_reconnect_delay_ms`
- **Give up:** After `max_reconnect_attempts` (if set), emit `ConnectionFailed` error

### Stream Queue

- When `concurrentStreams` limit (from ServerInfo) is reached, new stream requests queue internally
- **Queue type:** Unbounded - caller responsible for managing request rate
- Queued requests dispatch automatically when stream slots free up

### Stream Cleanup

- When caller drops `mpsc::Receiver`, client detects this and sends `StopStream` to server
- Frees server resources and stream slot for queued requests

## Observability

- Use `#[tracing::instrument(skip_all)]` on all public methods
- No inline log statements (per code-cleanup.md guidelines)
- Spans provide context for debugging

## Integration with worker-service

### Shared Singleton Pattern

```rust
// In worker-service
pub struct WorkerServiceState {
    pub titan_client: Arc<TitanClient>,
    // ... other fields
}

// At startup (eager + retry)
let titan_client = Arc::new(
    TitanClient::new(TitanConfig {
        url: env::var("TITAN_URL")?,
        token: env::var("TITAN_TOKEN")?,
        max_reconnect_delay_ms: 30_000,
        max_reconnect_attempts: None, // unlimited
        auto_wrap_sol: false,
    }).await?
);
```

### Usage in Handlers

```rust
async fn handle_swap(
    State(state): State<Arc<WorkerServiceState>>,
) -> Result<impl IntoResponse, Error> {
    let quote = state.titan_client.get_swap_price(request).await?;
    let instructions = state.titan_client.get_swap_instructions(&quote, &user_pubkey).await?;

    // Build transaction using instructions.data, instructions.accounts, etc.
    // Same pattern as JupiterHelper
}
```

## CLI Test Binary

### Usage

```bash
# Environment variables (defaults)
export TITAN_URL="wss://api.titan.ag/api/v1/ws"
export TITAN_TOKEN="your-jwt-token"

# Commands
titan-cli info                          # GetInfo
titan-cli venues                        # GetVenues
titan-cli providers                     # ListProviders
titan-cli price SOL USDC 1.0            # GetSwapPrice (defaults to SOL/USDC)
titan-cli quote SOL USDC 1.0            # Single quote
titan-cli stream SOL USDC 1.0           # Stream quotes (Ctrl+C to stop)
titan-cli instructions SOL USDC 1.0     # Get full instructions output

# Override with CLI flags
titan-cli --url wss://... --token xxx price SOL USDC 1.0
```

### Default Behavior

- Default pair: SOL/USDC
- Default amount: 1 SOL
- Config: Environment variables with CLI flag overrides

## Code Style (per code-cleanup.md)

- **File size:** ~200 lines max per file
- **Visibility:** `pub` everywhere
- **Async:** Native `async fn`, no boxed futures
- **Errors:** Never panic, always return `Result`
- **Logging:** `#[instrument]` spans only, no inline logs
- **Documentation:** Minimal, only non-obvious behavior
- **Constants:** Centralized in module or `constants.rs`

## Testing Strategy

### Mock WebSocket Server

Create an in-process mock Titan server for local testing that mimics real API behavior:

```rust
// tests/mock_server.rs
pub struct MockTitanServer {
    addr: SocketAddr,
    handle: JoinHandle<()>,
}

impl MockTitanServer {
    /// Start a mock server on a random port
    pub async fn start() -> Self;

    /// Get the WebSocket URL to connect to
    pub fn url(&self) -> String;

    /// Configure mock responses
    pub fn set_server_info(&self, info: ServerInfo);
    pub fn set_venues(&self, venues: VenueInfo);
    pub fn set_providers(&self, providers: Vec<ProviderInfo>);

    /// Configure stream behavior
    pub fn set_max_concurrent_streams(&self, max: u32);
    pub fn emit_quote(&self, stream_id: u32, quote: SwapQuotes);

    /// Simulate disconnection for reconnect testing
    pub fn disconnect(&self);

    /// Shutdown the server
    pub async fn stop(self);
}
```

### Test Scenarios

1. **Connection Tests:**
   - Connect to mock server
   - Handle authentication failure (401)
   - Test auto-reconnect on disconnect
   - Verify exponential backoff timing

2. **One-shot API Tests:**
   - `get_info()` returns expected data
   - `get_venues()` returns venues list
   - `list_providers()` returns providers
   - `get_swap_price()` with valid/invalid params

3. **Streaming Tests:**
   - Start stream, receive quotes
   - Multiple concurrent streams
   - Stream queue when at limit
   - Stream cleanup on drop
   - Stream resumption after reconnect

4. **Integration Tests (requires real API):**
   - Real connection with valid token
   - End-to-end quote streaming
   - Instruction preparation with RPC

### Running Tests

```bash
# Unit tests with mock server
cargo test

# Integration tests (requires TITAN_TOKEN env var)
TITAN_TOKEN=xxx cargo test --features integration

# Specific test
cargo test test_reconnect_with_backoff
```

### Test Helpers

```rust
// tests/helpers.rs

/// Create a test config pointing to mock server
pub fn test_config(mock_url: &str) -> TitanConfig;

/// Wait for connection state with timeout
pub async fn wait_for_state(
    client: &TitanClient,
    expected: ConnectionState,
    timeout: Duration,
) -> bool;

/// Generate mock SwapQuotes for testing
pub fn mock_swap_quotes(in_amount: u64, out_amount: u64) -> SwapQuotes;
```

## Implementation Order

1. Set up crate structure with `titan-api-codec` and `titan-api-types` dependencies
2. Implement `TitanConfig` and `TitanClientError`
3. Implement basic WebSocket connection (no reconnect yet)
4. Add one-shot methods: `get_info`, `get_venues`, `list_providers`, `get_swap_price`
5. Add streaming: `new_swap_quote_stream` with channel delivery
6. Add `prepare_instructions` helper (fetch ALTs, convert types)
7. Add stream queue for concurrency limits
8. Add auto-reconnect with exponential backoff
9. Add stream resumption on reconnect
10. Add connection state observable
11. Add graceful shutdown
12. Build CLI binary
13. Write tests

## Type Re-exports and Feature Flags

Enable these `titan-api-types` features for Solana type conversions:
- `solana-instruction-convert-v2` or `solana-instruction-convert-v3` - Convert `Instruction` and `AccountMeta`
- `solana-pubkey-convert-v2` or `solana-pubkey-convert-v3` - Convert `Pubkey`
- `solana-alt-account-convert-v2` - Convert `AddressLookupTableAccount`

Re-export key types from `titan-api-types::ws::v1`:
- `SwapQuoteRequest`, `SwapParams`, `TransactionParams`, `QuoteUpdateParams`
- `SwapQuotes`, `SwapRoute`, `RoutePlanStep`
- `SwapPrice`, `SwapPriceRequest`
- `ServerInfo`, `VenueInfo`, `ProviderInfo`
- `SwapMode`

Re-export from `titan-api-types::common`:
- `Instruction`, `AccountMeta`, `Pubkey`
