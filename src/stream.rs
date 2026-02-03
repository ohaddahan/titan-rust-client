//! Stream management and types.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use titan_api_types::ws::v1::{RequestData, StopStreamRequest, SwapQuotes};
use tokio::sync::mpsc;

use crate::connection::Connection;
use crate::queue::StreamManager;

/// A handle to an active quote stream.
///
/// When dropped, automatically sends `StopStream` to the server.
pub struct QuoteStream {
    stream_id: u32,
    effective_stream_id: Arc<AtomicU32>,
    slot_released: Arc<AtomicBool>,
    receiver: mpsc::Receiver<SwapQuotes>,
    connection: Arc<Connection>,
    manager: Option<Arc<StreamManager>>,
    stopped: bool,
}

impl QuoteStream {
    /// Create a new quote stream handle (unmanaged, for backwards compatibility).
    pub fn new(
        stream_id: u32,
        receiver: mpsc::Receiver<SwapQuotes>,
        connection: Arc<Connection>,
    ) -> Self {
        Self {
            stream_id,
            effective_stream_id: Arc::new(AtomicU32::new(stream_id)),
            slot_released: Arc::new(AtomicBool::new(false)),
            receiver,
            connection,
            manager: None,
            stopped: false,
        }
    }

    /// Create a new managed quote stream handle.
    pub fn new_managed(
        stream_id: u32,
        receiver: mpsc::Receiver<SwapQuotes>,
        connection: Arc<Connection>,
        manager: Option<Arc<StreamManager>>,
        effective_stream_id: Arc<AtomicU32>,
        slot_released: Arc<AtomicBool>,
    ) -> Self {
        Self {
            stream_id,
            effective_stream_id,
            slot_released,
            receiver,
            connection,
            manager,
            stopped: false,
        }
    }

    /// Get the original stream ID assigned at creation.
    pub fn stream_id(&self) -> u32 {
        self.stream_id
    }

    /// Get the current effective stream ID (may differ after reconnection).
    pub fn effective_stream_id(&self) -> u32 {
        self.effective_stream_id.load(Ordering::SeqCst)
    }

    /// Receive the next quote update.
    ///
    /// Returns `None` when the stream ends.
    pub async fn recv(&mut self) -> Option<SwapQuotes> {
        self.receiver.recv().await
    }

    /// Explicitly stop the stream.
    ///
    /// This is called automatically on drop, but can be called manually
    /// if you want to handle the result.
    pub async fn stop(&mut self) -> Result<(), crate::error::TitanClientError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;

        let effective_id = self.effective_stream_id.load(Ordering::SeqCst);

        // Send stop request first so server releases the stream before we free the slot
        let _ = self
            .connection
            .send_request(RequestData::StopStream(StopStreamRequest {
                id: effective_id,
            }))
            .await;

        // Unregister from connection
        self.connection.unregister_stream(effective_id).await;

        // CAS guard: only the first path to release the slot wins
        if self
            .slot_released
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if let Some(ref manager) = self.manager {
                manager.stream_ended();
            }
        }

        Ok(())
    }
}

impl Drop for QuoteStream {
    fn drop(&mut self) {
        if !self.stopped {
            let effective_id = self.effective_stream_id.load(Ordering::SeqCst);
            let connection = self.connection.clone();
            let manager = self.manager.clone();
            let slot_released = self.slot_released.clone();

            tokio::spawn(async move {
                // Send stop request first so server releases the stream
                let _ = connection
                    .send_request(RequestData::StopStream(StopStreamRequest {
                        id: effective_id,
                    }))
                    .await;

                connection.unregister_stream(effective_id).await;

                // CAS guard: only the first path to release the slot wins
                if slot_released
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    if let Some(ref manager) = manager {
                        manager.stream_ended();
                    }
                }
            });
        }
    }
}
