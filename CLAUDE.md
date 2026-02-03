# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Rust client library for the Titan Exchange WebSocket API, providing:

- Real-time swap quote streaming via WebSocket
- One-shot price queries
- Auto-reconnection with exponential backoff
- Stream management with internal queuing
- Transaction instruction extraction for custom Solana transaction building

Designed as both a publishable lib crate and includes a CLI binary for testing.

## Build Commands

```bash
# Build
cargo build
cargo build --release

# Run CLI (requires --features cli)
cargo run --features cli --bin titan-cli -- --help
cargo run --features cli --bin titan-cli -- info
cargo run --features cli --bin titan-cli -- price SOL USDC 1000000000
cargo run --features cli --bin titan-cli -- stream SOL USDC 1000000000
cargo run --features cli --bin titan-cli -- swap --keypair ~/.config/solana/id.json SOL USDC 100000000
cargo run --features cli --bin titan-cli -- watch

# Tests
cargo test
cargo test --test [name]

# Lint and format
cargo fmt
cargo clippy --all-targets
```

## Architecture

### Core Components

**TitanClient**: Main client struct, holds WebSocket connection and manages streams. Thread-safe (Arc internally) for sharing across axum handlers. Stores `TitanConfig` for runtime settings like `one_shot_timeout_ms`, `ping_interval_ms`, `pong_timeout_ms`, and `danger_accept_invalid_certs`.

**Connection Management**: Background tokio task reads WebSocket messages, dispatches to stream channels. Auto-reconnects with exponential backoff on disconnect.

**Stream Queue**: When `concurrentStreams` limit is reached, new stream requests queue internally and dispatch when slots free up.

**Stream Slot Lifecycle**: Each stream occupies a concurrency slot tracked by `active_count` in `StreamManager`. A shared `Arc<AtomicBool>` (`slot_released`) guards each slot so that exactly one code path (user `stop()`/`Drop`, server `StreamEnd`, or reconnection failure) decrements the counter. The `on_end` callback in `ResumableStream` bridges the connection layer (which handles server-initiated endings) to the queue layer (which owns slot accounting). After reconnection, `effective_stream_id: Arc<AtomicU32>` is updated so that `QuoteStream` sends `StopStream` with the correct remapped ID.

### Key Files

- `src/lib.rs` - Library entry point, module declarations, type re-exports
- `src/client.rs` - TitanClient implementation (streaming + one-shot + connection management API)
- `src/config.rs` - TitanConfig struct with fluent builder (`one_shot_timeout_ms`, `ping_interval_ms`, `pong_timeout_ms`, `danger_accept_invalid_certs`, etc.)
- `src/connection.rs` - WebSocket connection management, ping/pong keepalive, stream resumption, `ResumableStream` with `on_end` callback
- `src/error.rs` - TitanClientError enum (Timeout, AuthenticationFailed, RateLimited, ConnectionFailed, ConnectionClosed, ServerError, WebSocket, Serialization, Deserialization, Unexpected)
- `src/state.rs` - ConnectionState enum (Connected, Reconnecting, Disconnected) with observer pattern
- `src/stream.rs` - QuoteStream handle with CAS slot guard
- `src/queue.rs` - StreamManager concurrency limiter and request queue
- `src/instructions.rs` - Solana instruction conversion: Titan routes → `solana-sdk` Instructions + ALT fetching (feature-gated: `solana`)
- `src/tls.rs` - TLS config builders: secure (native certs) + dangerous (accept-all verifier for dev)
- `src/bin/titan_cli.rs` - CLI binary with subcommands: info, venues, providers, price, stream, swap, watch

## Configuration

Environment variables for CLI:

- `TITAN_URL` - WebSocket URL (CLI default: `wss://us1.api.demo.titan.exchange/api/v1/ws`; library default: `wss://api.titan.ag/api/v1/ws`)
- `TITAN_TOKEN` - JWT authentication token (required)
- `SOLANA_RPC_URL` - Solana RPC endpoint (default: `https://api.mainnet-beta.solana.com`)
- `TITAN_DANGER_ACCEPT_INVALID_CERTS` - Accept invalid TLS certs (dev only)

### TitanConfig Fields

| Field | Default | Description |
|-------|---------|-------------|
| `url` | `wss://api.titan.ag/api/v1/ws` | WebSocket URL |
| `token` | (required) | JWT auth token |
| `max_reconnect_delay_ms` | 30,000 | Max backoff ceiling |
| `max_reconnect_attempts` | None (infinite) | Optional reconnect cap |
| `auto_wrap_sol` | false | Auto-wrap SOL |
| `danger_accept_invalid_certs` | false | Skip TLS cert verification (dev) |
| `ping_interval_ms` | (from connection defaults) | WebSocket ping interval |
| `pong_timeout_ms` | (from connection defaults) | Pong response timeout |
| `one_shot_timeout_ms` | 10,000 | Timeout for `get_swap_price` |

## Public API

### Streaming
- `new_swap_quote_stream(SwapQuoteRequest) -> Result<QuoteStream>` — long-lived stream, caller controls recv/stop
- `get_swap_price(SwapQuoteRequest) -> Result<SwapQuotes>` — one-shot wrapper: open stream → recv first quote → auto-stop. Uses `config.one_shot_timeout_ms` (default 10s).

### One-shot (no streaming)
- `get_swap_price_simple(SwapPriceRequest) -> Result<SwapPrice>` — simple request/response, no stream overhead
- `get_info() -> Result<ServerInfo>` — server info and connection limits
- `get_venues() -> Result<VenueInfo>` — available trading venues
- `list_providers() -> Result<Vec<ProviderInfo>>` — liquidity providers

### Connection Management
- `state() -> ConnectionState` — current connection state
- `state_receiver() -> watch::Receiver<ConnectionState>` — connection state observable
- `is_connected() -> bool` — quick connected check
- `wait_for_connected() -> Result<()>` — blocks until connected (or fails permanently)
- `close() -> Result<()>` — graceful shutdown: stops all streams, clears manager, closes WebSocket
- `is_closed() -> bool` — whether client has been closed

### Stream Introspection
- `active_stream_count() -> u32` — number of active concurrent streams
- `queued_stream_count() -> usize` — number of requests waiting for a free slot

### Solana Instructions (feature: `solana`)
- `TitanInstructions::prepare_instructions(route, rpc) -> Result<TitanInstructionsOutput>` — converts Titan route to solana-sdk Instructions + fetches ALTs
- `TitanInstructions::fetch_address_lookup_tables(addresses, rpc)` — fetch ALT accounts from chain
- `TitanInstructions::convert_instructions(instructions)` — batch convert Titan → solana-sdk instructions
- `TitanInstructions::convert_instruction(instruction)` — single instruction conversion

### Known server behavior
- The streaming endpoint may send initial `SwapQuotes` frames with empty `quotes` vector before real data arrives, especially with dummy user pubkeys. Tests should not assert `!quotes.quotes.is_empty()` unless using a real wallet address.

## Integration with worker-service

```rust
// Shared singleton in axum state
pub struct WorkerServiceState {
    pub titan_client: Arc<TitanClient>,
    // ...
}
```

## Coding Style

### File Organization

- **Maximum file size: ~200 lines** - Split larger files
- **mod.rs files should be empty** - Only contain mod declarations, move content to meaningful file names
- Use helper structs with static methods for clear separation (e.g., `TitanHelper::method()`)

### Error Handling

- Single error enum for the crate (`TitanClientError`)
- All fallible operations return `Result`
- **Never panic** - Always propagate errors via Result
- Use typed variants for recoverable errors (auth, rate limit), anyhow for unexpected

### Async Functions

- Use native `async fn` directly
- No boxed futures or `async-trait` unless absolutely necessary
- Tokio-only (no runtime-agnostic abstractions)

### Visibility

- Use `pub` everywhere for simplicity

### Logging & Observability

- Use `#[tracing::instrument(skip_all)]` on public methods
- **No inline log statements** - Spans provide enough context
- Remove `println!`, `dbg!`, and most `tracing::info!` statements
- Keep only `tracing::error!` and `tracing::warn!` for critical issues

### Documentation

- Minimal doc comments
- Only document non-obvious behavior
- Let names and types be self-documenting

### Constants

- Consolidate all constants into central location or `constants.rs`
- Remove magic numbers from inline code

### Type Re-exports

- Re-export `titan-api-types` types directly (no wrapper types)
- Enable Solana conversion features for seamless integration

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `solana` | yes | Solana SDK integration (`instructions.rs`, type conversions) |
| `cli` | no | CLI binary + deps (clap, dotenvy, tracing-subscriber, bs58, tokio-stream) |

`docs.rs` builds with `default-features = false` to avoid Solana deps timeout.

## Dependencies

### Core
- `titan-api-codec` (1.2.1) - MessagePack encoding/decoding with zstd/brotli/gzip compression
- `titan-api-types` (2.0.0) - API type definitions with Solana conversion features
- `tokio` - Async runtime (full features)
- `tokio-tungstenite` (0.26) - WebSocket client with rustls-tls-native-roots
- `rustls` (0.23) + `tokio-rustls` (0.26) + `rustls-native-certs` (0.8) - TLS stack
- `futures-util` / `tokio-util` - Async utilities
- `rmp-serde` / `serde` / `serde_json` / `bytes` - Serialization
- `thiserror` (2) / `anyhow` (1) - Error handling
- `tracing` - Observability

### Solana (optional, feature-gated)
- `solana-sdk` (2.3) - Transaction building
- `solana-client` (2.3) - RPC client for ALT fetching
- `solana-address-lookup-table-interface` (2.2.2) - ALT deserialization

### CLI (optional, feature-gated)
- `clap` (4) - Argument parsing
- `dotenvy` - Environment variables
- `bs58` - Base58 encoding
- `tokio-stream` - Stream utilities
- `tracing-subscriber` - Log output

## Further Reading

See `FOR_USER.md` for an in-depth narrative explanation of the architecture, design decisions, lessons learned, and pitfalls encountered during development.
