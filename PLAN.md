# Plan

## Scope (agreed)
- In-flight requests should fail immediately on disconnect (no retry).
- `QuoteStream` drop should be safe outside a Tokio runtime (best-effort, no panic).

## Findings to address
1) `QuoteStream` drop can panic if no runtime is active.
2) `close()` doesn’t stop the connection loop; WS can stay alive.
3) Pending requests can hang across disconnects (only drained at final shutdown).
4) Stream resumption assumes next frame is the resume response; can mis-handle interleaving.
5) Race between `Drop` and stream ID remap can send StopStream to stale ID and leak map entries.
6) Ping/pong timeout can fire before a scheduled ping is sent.
7) Queue processing spawns many redundant tasks under load.

## Proposed changes
1) **Safe drop path**
   - Guard `tokio::spawn` in `Drop` with `Handle::try_current`.
   - If no runtime, skip async cleanup; document that `stop()` is preferred for deterministic shutdown.

2) **Fail pending requests on disconnect**
   - Add `TitanClientError::ConnectionClosed` (or `Disconnected`) for this case.
   - When `run_single_connection` exits, immediately drain `pending_requests` with that error.
   - Ensure callers of `send_request` receive a fast failure on connection loss.

3) **Actually stop the connection loop on `close()`**
   - Treat `request_rx.recv()` returning `None` as shutdown in `run_single_connection`.
   - Optionally send a Close frame before dropping.

4) **Robust stream resumption**
   - While resuming, read frames until the response with matching `request_id`.
   - Pass unrelated frames back to the normal handler (responses, stream data, pings).

5) **Drop/remap race hardening**
   - Track a shared “stopped” flag for streams; skip resumption/remap when stopped.
   - In `stop()`/`Drop`, attempt unregister of both original and effective IDs as a safety net.

6) **Ping/pong policy**
   - Track “awaiting pong” after sending ping, and only time out if that pong doesn’t arrive.
   - Or enforce `pong_timeout_ms >= ping_interval_ms` and document it.

7) **Queue worker**
   - Spawn a single queue processor when the queue transitions empty → non-empty.
   - Avoid per-request task fan-out.

## Tests to add
- `close()` shuts down background task (server shows 0 connected clients within timeout).
- Pending request fails promptly after disconnect (no hang) with `ConnectionClosed`.
- Stream resumption with interleaved frames maps to new stream ID correctly.
- Dropping `QuoteStream` without a runtime does not panic (catch_unwind test).
- Resumption failure releases slot even if stream receiver is already closed.
- Queue worker processes burst of queued requests without spawning multiple processors.

## Order of work
1) Fail pending requests on disconnect + close shutdown handling.
2) Safe drop path.
3) Robust resumption loop + drop/remap race hardening.
4) Ping/pong policy tweak.
5) Queue worker change.
6) Add tests and run `cargo test` (and `./scripts/tests.sh` if you want ignored tests).
