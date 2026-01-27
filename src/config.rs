//! Configuration for the Titan client.

use crate::connection::{DEFAULT_PING_INTERVAL_MS, DEFAULT_PONG_TIMEOUT_MS};

#[derive(Debug, Clone)]
pub struct TitanConfig {
    pub url: String,
    pub token: String,
    pub max_reconnect_delay_ms: u64,
    pub max_reconnect_attempts: Option<u32>,
    pub auto_wrap_sol: bool,
    pub danger_accept_invalid_certs: bool,
    pub ping_interval_ms: u64,
    pub pong_timeout_ms: u64,
}

impl TitanConfig {
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            max_reconnect_delay_ms: 30_000,
            max_reconnect_attempts: None,
            auto_wrap_sol: false,
            danger_accept_invalid_certs: false,
            ping_interval_ms: DEFAULT_PING_INTERVAL_MS,
            pong_timeout_ms: DEFAULT_PONG_TIMEOUT_MS,
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

    pub fn with_danger_accept_invalid_certs(mut self, accept: bool) -> Self {
        self.danger_accept_invalid_certs = accept;
        self
    }

    pub fn with_ping_interval_ms(mut self, ms: u64) -> Self {
        self.ping_interval_ms = ms;
        self
    }

    pub fn with_pong_timeout_ms(mut self, ms: u64) -> Self {
        self.pong_timeout_ms = ms;
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
            danger_accept_invalid_certs: false,
            ping_interval_ms: DEFAULT_PING_INTERVAL_MS,
            pong_timeout_ms: DEFAULT_PONG_TIMEOUT_MS,
        }
    }
}
