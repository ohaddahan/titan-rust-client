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

# Run CLI
cargo run --bin titan-cli -- --help
cargo run --bin titan-cli -- info
cargo run --bin titan-cli -- quote SOL USDC 1.0

# Tests
cargo test
cargo test --test [name]

# Lint and format
cargo fmt
cargo clippy --all-targets
```

## Architecture

### Core Components

**TitanClient**: Main client struct, holds WebSocket connection and manages streams. Thread-safe (Arc internally) for sharing across axum handlers.

**Connection Management**: Background tokio task reads WebSocket messages, dispatches to stream channels. Auto-reconnects with exponential backoff on disconnect.

**Stream Queue**: When `concurrentStreams` limit is reached, new stream requests queue internally and dispatch when slots free up.

### Key Files

- `src/lib.rs` - Library entry point, re-exports
- `src/client.rs` - TitanClient implementation
- `src/config.rs` - TitanConfig struct
- `src/connection.rs` - WebSocket connection management
- `src/error.rs` - TitanClientError enum
- `src/state.rs` - Connection state observable
- `src/stream.rs` - Stream management and queue
- `src/bin/titan-cli.rs` - CLI test binary

## Configuration

Environment variables for CLI:

- `TITAN_URL` - WebSocket URL (default: wss://api.titan.ag/api/v1/ws)
- `TITAN_TOKEN` - JWT authentication token

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

## Dependencies

### Core
- `titan-api-codec` - MessagePack encoding/decoding
- `titan-api-types` - API type definitions
- `tokio` - Async runtime
- `tokio-tungstenite` - WebSocket client
- `thiserror` / `anyhow` - Error handling
- `tracing` - Observability

### CLI
- `clap` - Argument parsing
- `dotenvy` - Environment variables
