//! Stream management and types.

use std::sync::Arc;

use titan_api_types::ws::v1::{RequestData, StopStreamRequest, SwapQuotes};
use tokio::sync::mpsc;

use crate::connection::Connection;

/// A handle to an active quote stream.
///
/// When dropped, automatically sends `StopStream` to the server.
pub struct QuoteStream {
    stream_id: u32,
    receiver: mpsc::Receiver<SwapQuotes>,
    connection: Arc<Connection>,
    stopped: bool,
}

impl QuoteStream {
    /// Create a new quote stream handle.
    pub fn new(
        stream_id: u32,
        receiver: mpsc::Receiver<SwapQuotes>,
        connection: Arc<Connection>,
    ) -> Self {
        Self {
            stream_id,
            receiver,
            connection,
            stopped: false,
        }
    }

    /// Get the stream ID.
    pub fn stream_id(&self) -> u32 {
        self.stream_id
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

        // Unregister from connection
        self.connection.unregister_stream(self.stream_id).await;

        // Send stop request (fire and forget - we don't care about the response)
        let _ = self
            .connection
            .send_request(RequestData::StopStream(StopStreamRequest {
                id: self.stream_id,
            }))
            .await;

        Ok(())
    }
}

impl Drop for QuoteStream {
    fn drop(&mut self) {
        if !self.stopped {
            let stream_id = self.stream_id;
            let connection = self.connection.clone();

            // Spawn a task to stop the stream
            tokio::spawn(async move {
                connection.unregister_stream(stream_id).await;
                let _ = connection
                    .send_request(RequestData::StopStream(StopStreamRequest { id: stream_id }))
                    .await;
            });
        }
    }
}
