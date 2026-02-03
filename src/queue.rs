//! Stream queue management for concurrency limits.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use titan_api_types::ws::v1::{RequestData, StreamDataPayload, SwapQuoteRequest, SwapQuotes};
use tokio::sync::{mpsc, oneshot, Mutex, Notify};

use crate::connection::Connection;
use crate::error::TitanClientError;
use crate::stream::QuoteStream;

/// Queued stream request waiting to be started.
struct QueuedRequest {
    request: SwapQuoteRequest,
    result_tx: oneshot::Sender<Result<QuoteStream, TitanClientError>>,
}

/// Manages stream concurrency and queuing.
pub struct StreamManager {
    max_concurrent: AtomicU32,
    active_count: AtomicU32,
    queue: Mutex<VecDeque<QueuedRequest>>,
    connection: Arc<Connection>,
    slot_available: Notify,
}

impl StreamManager {
    /// Create a new stream manager.
    pub fn new(connection: Arc<Connection>, max_concurrent: u32) -> Arc<Self> {
        Arc::new(Self {
            max_concurrent: AtomicU32::new(max_concurrent),
            active_count: AtomicU32::new(0),
            queue: Mutex::new(VecDeque::new()),
            connection,
            slot_available: Notify::new(),
        })
    }

    /// Update the max concurrent streams limit.
    pub fn set_max_concurrent(&self, max: u32) {
        self.max_concurrent.store(max, Ordering::SeqCst);
        // Notify in case we can now start more streams
        self.slot_available.notify_waiters();
    }

    /// Get current active stream count.
    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    /// Get current queue length.
    pub async fn queue_len(&self) -> usize {
        self.queue.lock().await.len()
    }

    /// Request a new stream. May wait in queue if at concurrency limit.
    #[tracing::instrument(skip_all)]
    pub async fn request_stream(
        self: &Arc<Self>,
        request: SwapQuoteRequest,
    ) -> Result<QuoteStream, TitanClientError> {
        // Try to start immediately if under limit
        let max = self.max_concurrent.load(Ordering::SeqCst);
        let current = self.active_count.load(Ordering::SeqCst);

        if current < max {
            // Try to claim a slot
            if self
                .active_count
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return self.start_stream_internal(request).await;
            }
        }

        // Queue the request and wait
        let (result_tx, result_rx) = oneshot::channel();
        {
            let mut queue = self.queue.lock().await;
            queue.push_back(QueuedRequest { request, result_tx });
        }

        // Spawn task to process queue when slot becomes available
        let manager = self.clone();
        tokio::spawn(async move {
            manager.process_queue().await;
        });

        // Wait for our turn
        result_rx.await.map_err(|_| {
            TitanClientError::Unexpected(anyhow::anyhow!("Stream request cancelled"))
        })?
    }

    /// Called when a stream ends to free up a slot.
    pub fn stream_ended(&self) {
        self.active_count.fetch_sub(1, Ordering::SeqCst);
        self.slot_available.notify_one();
    }

    /// Process queued requests when slots become available.
    async fn process_queue(self: &Arc<Self>) {
        loop {
            let max = self.max_concurrent.load(Ordering::SeqCst);
            let current = self.active_count.load(Ordering::SeqCst);

            if current >= max {
                // Wait for a slot
                self.slot_available.notified().await;
                continue;
            }

            // Try to get next queued request
            let queued = {
                let mut queue = self.queue.lock().await;
                queue.pop_front()
            };

            let Some(queued) = queued else {
                // Queue empty, done
                break;
            };

            // Claim a slot
            if self
                .active_count
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                // Lost race, re-queue and retry
                let mut queue = self.queue.lock().await;
                queue.push_front(queued);
                continue;
            }

            // Start the stream
            let result = self.start_stream_internal(queued.request).await;
            let _ = queued.result_tx.send(result);
        }
    }

    /// Internal: actually start a stream (slot must already be claimed).
    async fn start_stream_internal(
        self: &Arc<Self>,
        request: SwapQuoteRequest,
    ) -> Result<QuoteStream, TitanClientError> {
        // Pre-create the slot guard so cleanup paths can use it
        let slot_released = Arc::new(AtomicBool::new(false));

        let response = self
            .connection
            .send_request(RequestData::NewSwapQuoteStream(request.clone()))
            .await
            .inspect_err(|_| {
                // Release slot on error via CAS guard
                if slot_released
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    self.active_count.fetch_sub(1, Ordering::SeqCst);
                    self.slot_available.notify_one();
                }
            })?;

        let stream_id = response
            .stream
            .ok_or_else(|| {
                if slot_released
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    self.active_count.fetch_sub(1, Ordering::SeqCst);
                    self.slot_available.notify_one();
                }
                TitanClientError::Unexpected(anyhow::anyhow!(
                    "NewSwapQuoteStream response missing stream info"
                ))
            })?
            .id;

        // Shared atomic for the current server-side stream ID (updated on reconnect)
        let effective_stream_id = Arc::new(AtomicU32::new(stream_id));

        // Build the on_end callback for server-side cleanup paths
        let on_end_slot_released = slot_released.clone();
        let on_end_manager: Arc<Self> = self.clone();
        let on_end: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if on_end_slot_released
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                on_end_manager.stream_ended();
            }
        });

        // Create channels for stream data
        let (raw_tx, mut raw_rx) = mpsc::channel::<titan_api_types::ws::v1::StreamData>(32);
        let (quotes_tx, quotes_rx) = mpsc::channel::<SwapQuotes>(32);

        // Register the raw stream with the connection (includes request for resumption)
        self.connection
            .register_stream(
                stream_id,
                request,
                raw_tx,
                Some(on_end),
                Some(effective_stream_id.clone()),
            )
            .await;

        // Spawn adapter task
        let adapter_connection = self.connection.clone();
        let adapter_effective_id = effective_stream_id.clone();
        tokio::spawn(async move {
            while let Some(data) = raw_rx.recv().await {
                match data.payload {
                    StreamDataPayload::SwapQuotes(quotes) => {
                        if quotes_tx.send(quotes).await.is_err() {
                            let eid = adapter_effective_id.load(Ordering::SeqCst);
                            adapter_connection.unregister_stream(eid).await;
                            break;
                        }
                    }
                    StreamDataPayload::Other(_) => {
                        tracing::warn!("Received unexpected stream data payload type");
                    }
                }
            }
        });

        Ok(QuoteStream::new_managed(
            stream_id,
            quotes_rx,
            self.connection.clone(),
            Some(self.clone()),
            effective_stream_id,
            slot_released,
        ))
    }
}
