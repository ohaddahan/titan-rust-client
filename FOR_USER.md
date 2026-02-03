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
┌─────────────┐
│ TitanClient  │  ← Your entry point. Thread-safe, shareable via Arc.
├─────────────┤
│ StreamManager│  ← Enforces concurrency limits. Queues excess requests.
├─────────────┤
│ Connection   │  ← WebSocket I/O. Background task for read loop.
│              │     Auto-reconnects with exponential backoff.
│              │     Resumes streams after reconnect.
├─────────────┤
│ QuoteStream  │  ← Handle you hold. recv() for quotes, stop() when done.
│              │     Drop impl cleans up automatically.
└─────────────┘
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

`TitanClientError` uses `thiserror` for typed variants (Timeout, AuthenticationFailed, RateLimited) that callers can match on. The `Unexpected` variant wraps `anyhow::Error` for everything else. This is a practical pattern: structure what you can, bag the rest. Callers who need to handle specific errors match the typed variants; everything else bubbles up as an opaque error with context.

## Technology Choices

| Choice | Why |
|--------|-----|
| `tokio-tungstenite` | Native async WebSocket client for tokio. No need for `async-tungstenite` or runtime-agnostic abstractions — this crate is tokio-only. |
| `rmp-serde` (MessagePack) | Titan's wire protocol is MessagePack, not JSON. Binary format = smaller payloads, faster serialization. The `titan-api-codec` crate wraps this. |
| `tracing` over `log` | Structured spans > flat log lines. `#[tracing::instrument(skip_all)]` on public methods gives you call-tree visibility without manually logging entry/exit. |
| `rustls` over `native-tls` | Pure Rust TLS. No OpenSSL dependency, simpler cross-compilation. The `danger_accept_invalid_certs` option exists for dev/test environments. |
| No `async-trait` | Native `async fn` in traits landed in Rust 1.75. This crate uses native async throughout — no boxing overhead. |
