# Titan Rust Client — The Story Behind the Code

This document explains how the Titan Rust Client works, why it's built the way it is, and what you can learn from it. Think of it as the "director's commentary" for the codebase.

## What Does This Thing Do?

Titan is a DEX aggregator on Solana — it finds the best swap routes across multiple liquidity providers. This crate is the Rust client that talks to Titan's WebSocket API.

There are two ways to get swap quotes:

1. **"What's the price?"** — A quick request/response. You ask "how much USDC for 1 SOL?", the server answers immediately. Simple, stateless. (`get_swap_price_simple`)

2. **"Keep me updated"** — A persistent stream. The server continuously pushes updated quotes as market conditions change. You pick the best one when you're ready to execute. (`new_swap_quote_stream`)

3. **"Just give me one good quote"** — A convenience wrapper that opens a stream, grabs the first quote with transaction instructions, and auto-closes. Best of both worlds when you don't need continuous updates. (`get_swap_price`)

## Architecture at a Glance

```
┌──────────────────┐
│   TitanClient     │  ← Your entry point. Thread-safe, shareable via Arc.
├──────────────────┤
│   StreamManager   │  ← Enforces concurrency limits. Queues excess requests.
├──────────────────┤
│   Connection      │  ← WebSocket I/O. Background task for read loop.
│                   │     Auto-reconnects with exponential backoff.
│                   │     Resumes streams after reconnect.
│                   │     Ping/pong keepalive for connection health.
├──────────────────┤
│   QuoteStream     │  ← Handle you hold. recv() for quotes, stop() when done.
│                   │     Drop impl cleans up automatically.
├──────────────────┤
│ TitanInstructions │  ← Converts Titan routes → solana-sdk Instructions.
│   (feature-gated) │     Fetches ALTs from chain for versioned transactions.
├──────────────────┤
│   TLS Layer       │  ← rustls-based. Secure (native certs) or dangerous
│                   │     (accept-all verifier) for dev environments.
└──────────────────┘
```

**Data flow for streaming:**
```
Server → WebSocket → Connection (background task)
  → dispatches StreamData by stream_id
  → adapter task decodes StreamDataPayload::SwapQuotes
  → mpsc channel → QuoteStream.recv()
```

## The Hardest Problem: Stream Slot Accounting

The Titan server limits how many concurrent streams you can have (typically ~10). The client tracks this with `active_count`. The tricky part: a stream can end in three different ways, and each path must decrement `active_count` exactly once.

**The three paths:**
1. **You call `stream.stop()`** — explicit cleanup
2. **You drop the `QuoteStream`** — the `Drop` impl spawns an async cleanup task
3. **The server ends the stream** — sends `StreamEnd`, or the stream fails to resume after reconnect

If two paths fire simultaneously (say the server sends `StreamEnd` right as you call `stop()`), you get a double-decrement and your slot counter goes negative. Now the client thinks it has free slots when it doesn't, or worse, arithmetic underflow.

**The fix:** A compare-and-swap (CAS) guard using `Arc<AtomicBool>`. Each stream gets a `slot_released` flag initialized to `false`. The first path to successfully CAS it from `false` → `true` wins and gets to decrement. Everyone else sees `true` and skips. Simple, lock-free, correct.

```
slot_released: Arc<AtomicBool> = false

stop()     ──┐
Drop       ──┼── CAS(false → true) ── only winner decrements active_count
on_end()   ──┘
```

This is a pattern worth knowing. Anytime you have a resource counter that multiple code paths can release, CAS guards prevent double-free bugs without locks.

## Reconnection and Stream Resumption

When the WebSocket disconnects, the client doesn't just reconnect — it **resumes all active streams**. Here's what happens:

1. Connection drops (network blip, server restart, etc.)
2. Background task detects the drop, enters reconnection loop
3. Exponential backoff: 100ms → 200ms → 400ms → ... → max 30s
4. On successful reconnect, iterates `resumable_streams` map
5. Re-sends each `NewSwapQuoteStream` request to the server
6. Server assigns **new** stream IDs (old ones are gone)
7. Client updates `effective_stream_id: Arc<AtomicU32>` so `QuoteStream` uses the new ID for `StopStream`

This is why `QuoteStream` has both `stream_id` (original, never changes) and `effective_stream_id` (atomic, updated on reconnect). When you call `stream.stop()`, it reads the effective ID to send the correct `StopStream` to the server.

The `on_end` callback in `ResumableStream` is the bridge between layers: the connection layer detects server-initiated endings, the callback notifies the queue layer to release the slot. Clean separation of concerns.

## The `get_swap_price` Wrapper

This method looks simple but has subtle design choices:

```rust
pub async fn get_swap_price(&self, request: SwapQuoteRequest) -> Result<SwapQuotes> {
    let mut stream = self.new_swap_quote_stream(request).await?;
    let result = tokio::time::timeout(timeout, stream.recv()).await;
    match result {
        Ok(Some(quotes)) => { stream.stop().await; Ok(quotes) }
        Ok(None) => Err(stream_ended_error),
        Err(_) => { stream.stop().await; Err(Timeout) }
    }
}
```

**Why it occupies a concurrency slot:** It goes through the same `StreamManager` as long-lived streams. This means it respects the server's concurrent stream limit. If you're running 10 long-lived streams and call `get_swap_price`, it queues instead of violating the limit.

**Why `stream.stop()` even in the timeout path:** Without explicit stop, the `Drop` impl would spawn a task to clean up. That works, but explicit stop is synchronous (waits for the server acknowledgment). In a hot path, you want deterministic cleanup, not fire-and-forget.

**Why `let _ = stream.stop()`:** If stop fails (connection already dead), we don't care — the `on_end` callback or Drop will handle it. The CAS guard ensures exactly one path releases the slot regardless.

## The Solana Instruction Pipeline

Once you have a `SwapRoute` from the streaming API, you need to turn it into something Solana understands. That's what `instructions.rs` handles (behind the `solana` feature gate).

The pipeline:

```
SwapRoute (from Titan)
  │
  ├── route.address_lookup_tables  →  fetch from chain via RPC
  │     (list of ALT pubkeys)         → deserialize into AddressLookupTableAccount
  │
  ├── route.instructions           →  convert Titan types → solana-sdk types
  │     (Titan Instruction format)     (program_id, accounts, data mapping)
  │
  └── route.compute_units_safe     →  pass to ComputeBudgetInstruction
        (recommended CU budget)
```

**Why this exists separately:** You might think "just serialize and send." But Solana versioned transactions need `AddressLookupTableAccount` objects (with the full address list, not just the pubkey). Those have to be fetched live from chain. And the Titan API types have their own `Pubkey`/`Instruction`/`AccountMeta` — they need converting to `solana-sdk` equivalents via the `.into()` conversions that `titan-api-types` provides with its feature flags.

`TitanInstructions` is a stateless helper struct with static methods — it doesn't own any resources. You bring your own `RpcClient` and `SwapRoute`, it gives you back transaction-ready output.

## The TLS Layer

The `tls.rs` module provides two builders:

1. **`build_default_tls_config()`** — Loads the system's native root certificates via `rustls-native-certs`, builds a standard `rustls::ClientConfig`. This is what you use in production.

2. **`build_dangerous_tls_config()`** — Implements a `ServerCertVerifier` that accepts any certificate. Signature verification still happens (so TLS handshakes complete correctly), but server identity is not validated. Strictly for dev/test against local servers with self-signed certs.

A `LazyLock<Arc<CryptoProvider>>` ensures the ring crypto provider is initialized exactly once, avoiding the "provider already installed" panic that can happen if multiple parts of your app try to set the default.

## Connection State and Observability

The client exposes its connection health through `ConnectionState`:

```
Connected ←──→ Reconnecting { attempt: N }
    │                    │
    └────────────────────┘
              │
              ▼
    Disconnected { reason }
```

You can observe state changes via `client.state_receiver()`, which returns a `tokio::sync::watch::Receiver`. This is useful for health checks in axum handlers or for UI status indicators. The `wait_for_connected()` method blocks until the connection is established or fails permanently.

## Ping/Pong Keepalive

WebSocket connections can silently go stale — the server thinks you're connected, but packets are being dropped somewhere in between. The connection layer sends periodic WebSocket ping frames at `config.ping_interval_ms` intervals. If the server doesn't respond with a pong within `config.pong_timeout_ms`, the client treats the connection as dead and enters the reconnection loop.

This is a common pattern in production WebSocket clients and catches a class of failures that TCP keepalive alone misses (especially behind load balancers and proxies that may hold connections open indefinitely).

## The CLI Tool

The `titan_cli.rs` binary (behind the `cli` feature) provides hands-on access to every API:

| Command | What it does |
|---------|-------------|
| `info` | Fetch server info, protocol version, concurrent stream limits |
| `venues` | List trading venues with program IDs |
| `providers` | List liquidity providers with icons |
| `price` | One-shot price check (simple request/response) |
| `stream` | Open a live quote stream, print updates as they arrive |
| `swap` | Full swap execution: stream quote → pick best → build tx → sign → send |
| `watch` | Monitor connection state transitions in real-time |

The CLI supports token shortcuts (`SOL`, `USDC`, `USDT`) and resolves them to mint addresses. The `swap` command can run interactively (asks for confirmation) or automated with `--yes`.

## Lessons Learned

### 1. The "Empty Quotes" Surprise

The streaming endpoint sends `SwapQuotes` frames with correct metadata (`input_mint`, `output_mint`) but an empty `quotes` vector when using a dummy public key. This makes sense in retrospect: the server needs a real wallet address to construct transaction instructions (compute budget, account lookups, etc.). Without a valid pubkey, it can establish the stream but can't generate actual swap routes.

**Takeaway:** When testing against real APIs, don't assume all response fields will be populated just because the response itself succeeded. Validate at the boundary — check what the server actually returns before writing assertions.

### 2. CAS Guards > Mutexes for Single-Decrement Patterns

We could have used a `Mutex<bool>` for the slot release guard. CAS via `AtomicBool::compare_exchange` is better here because:
- No lock contention between the three release paths
- No risk of holding a lock across an await point
- Single atomic operation — fastest possible check-and-set

### 3. The Builder Pattern for Config

`TitanConfig` uses the fluent builder pattern (`with_*` methods that take and return `self`). This is idiomatic Rust for optional configuration. The `one_shot_timeout_ms` field defaulting to 10s means callers don't need to think about it unless they have specific latency requirements.

### 4. Separation of Connection and Stream Concerns

The codebase separates concerns across layers:
- `Connection` handles WebSocket I/O and message routing
- `StreamManager` handles concurrency limits and queuing
- `QuoteStream` is the user-facing handle

Each layer has a single responsibility. The `on_end` callback is the glue between them — it's a closure that captures the queue layer's state and gets called from the connection layer. This avoids circular dependencies between modules.

### 5. Why `thiserror` + `anyhow` Together

`TitanClientError` uses `thiserror` for typed variants (Timeout, AuthenticationFailed, RateLimited, ConnectionFailed, ConnectionClosed, ServerError, WebSocket, Serialization, Deserialization) that callers can match on. The `Unexpected` variant wraps `anyhow::Error` for everything else. This is a practical pattern: structure what you can, bag the rest. Callers who need to handle specific errors match the typed variants; everything else bubbles up as an opaque error with context.

### 6. Feature Gates and docs.rs Timeouts

The Solana SDK dependencies are heavy — they pull in a massive dependency tree. Building them on docs.rs's constrained CI runners was timing out. The fix: make Solana optional via `[features] default = ["solana"]` and set `[package.metadata.docs.rs] default-features = false`. Users get Solana support by default, but docs.rs can build without it.

The `instructions.rs` module is wrapped in `#[cfg(feature = "solana")]` in `lib.rs`. This is a clean pattern: the core streaming/querying functionality works without Solana at all.

### 7. Connection Reliability is Harder Than It Looks

PR #40 ("fix connection issues") revealed that a streaming feature wasn't working correctly and had to be removed. Real-world WebSocket connections fail in subtle ways: silent stale connections, reconnects that race with in-flight requests, stream IDs that change after reconnect. The ping/pong keepalive mechanism was added specifically because TCP keepalive alone doesn't catch all failure modes (especially behind cloud load balancers).

The lesson: when building clients for stateful protocols (WebSocket streams with server-assigned IDs), the reconnection logic is often more complex than the happy-path code. Budget accordingly.

### 8. LazyLock for Global Singletons

The TLS module uses `LazyLock<Arc<CryptoProvider>>` to initialize the rustls crypto provider exactly once. This avoids a subtle bug: if you call `install_default()` from multiple threads or at multiple call sites, the second call panics. `LazyLock` guarantees one-time initialization with no runtime cost after the first access — cleaner than `Once` + `Option` or calling `install_default()` and ignoring the result.

## Technology Choices

| Choice | Why |
|--------|-----|
| `tokio-tungstenite` | Native async WebSocket client for tokio. No need for `async-tungstenite` or runtime-agnostic abstractions — this crate is tokio-only. |
| `rmp-serde` (MessagePack) | Titan's wire protocol is MessagePack, not JSON. Binary format = smaller payloads, faster serialization. The `titan-api-codec` crate wraps this. |
| `tracing` over `log` | Structured spans > flat log lines. `#[tracing::instrument(skip_all)]` on public methods gives you call-tree visibility without manually logging entry/exit. |
| `rustls` over `native-tls` | Pure Rust TLS. No OpenSSL dependency, simpler cross-compilation. The `danger_accept_invalid_certs` option exists for dev/test environments. |
| `rustls-native-certs` | Loads system root CA certificates so TLS works out of the box on any OS without bundling certs. |
| No `async-trait` | Native `async fn` in traits landed in Rust 1.75. This crate uses native async throughout — no boxing overhead. |
| `solana-sdk` v2.3 | Latest Solana SDK with versioned transactions and ALT support. Feature-gated so non-Solana users don't pay the compile cost. |
| `cargo-husky` | Git pre-commit hooks that run `cargo test`, `cargo clippy`, and `cargo fmt` automatically. Catches issues before they reach CI. |
