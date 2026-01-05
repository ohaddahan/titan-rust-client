//! Connection state observable.

use std::fmt;

/// Connection state for observing connection health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connected to the server and ready for requests.
    Connected,

    /// Attempting to reconnect after disconnection.
    Reconnecting {
        /// Current reconnection attempt number.
        attempt: u32,
    },

    /// Disconnected from the server.
    Disconnected {
        /// Reason for disconnection.
        reason: String,
    },
}

impl ConnectionState {
    /// Returns true if currently connected.
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Returns true if currently reconnecting.
    pub fn is_reconnecting(&self) -> bool {
        matches!(self, Self::Reconnecting { .. })
    }

    /// Returns true if disconnected (not reconnecting).
    pub fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected { .. })
    }

    /// Returns the reconnection attempt number if reconnecting.
    pub fn reconnect_attempt(&self) -> Option<u32> {
        match self {
            Self::Reconnecting { attempt } => Some(*attempt),
            _ => None,
        }
    }

    /// Returns the disconnection reason if disconnected.
    pub fn disconnect_reason(&self) -> Option<&str> {
        match self {
            Self::Disconnected { reason } => Some(reason),
            _ => None,
        }
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Disconnected {
            reason: "Not connected".to_string(),
        }
    }
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connected => write!(f, "Connected"),
            Self::Reconnecting { attempt } => write!(f, "Reconnecting (attempt {})", attempt),
            Self::Disconnected { reason } => write!(f, "Disconnected: {}", reason),
        }
    }
}
