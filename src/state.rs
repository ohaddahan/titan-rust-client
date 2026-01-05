//! Connection state observable.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connected to the server
    Connected,

    /// Attempting to reconnect
    Reconnecting { attempt: u32 },

    /// Disconnected from the server
    Disconnected { reason: String },
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Disconnected {
            reason: "Not connected".to_string(),
        }
    }
}
