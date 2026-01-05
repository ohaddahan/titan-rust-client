//! Configuration for the Titan client.

#[derive(Debug, Clone)]
pub struct TitanConfig {
    /// WebSocket URL (e.g., wss://api.titan.ag/api/v1/ws)
    pub url: String,

    /// JWT token for authentication
    pub token: String,

    /// Maximum reconnection delay in milliseconds (default: 30000)
    pub max_reconnect_delay_ms: u64,

    /// Maximum reconnection attempts before giving up (default: unlimited/None)
    pub max_reconnect_attempts: Option<u32>,

    /// Whether to wrap/unwrap SOL automatically (default: false)
    pub auto_wrap_sol: bool,
}

impl TitanConfig {
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            max_reconnect_delay_ms: 30_000,
            max_reconnect_attempts: None,
            auto_wrap_sol: false,
        }
    }

    pub fn with_max_reconnect_delay_ms(mut self, ms: u64) -> Self {
        self.max_reconnect_delay_ms = ms;
        self
    }

    pub fn with_max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.max_reconnect_attempts = Some(attempts);
        self
    }

    pub fn with_auto_wrap_sol(mut self, enabled: bool) -> Self {
        self.auto_wrap_sol = enabled;
        self
    }
}

impl Default for TitanConfig {
    fn default() -> Self {
        Self {
            url: "wss://api.titan.ag/api/v1/ws".to_string(),
            token: String::new(),
            max_reconnect_delay_ms: 30_000,
            max_reconnect_attempts: None,
            auto_wrap_sol: false,
        }
    }
}
