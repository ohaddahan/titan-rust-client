//! Common test utilities and mock server.

pub mod mock_server;

pub use mock_server::MockTitanServer;

use std::time::Duration;
use titan_rust_client::{ConnectionState, TitanClient, TitanConfig};

/// Create a test config pointing to the mock server.
#[allow(dead_code)]
pub fn test_config(url: &str) -> TitanConfig {
    TitanConfig {
        url: url.to_string(),
        token: "test-token".to_string(),
        max_reconnect_delay_ms: 1000,
        max_reconnect_attempts: Some(3),
        auto_wrap_sol: false,
        danger_accept_invalid_certs: true,
        ping_interval_ms: 5_000,
        pong_timeout_ms: 10_000,
        one_shot_timeout_ms: 10_000,
    }
}

/// Wait for a specific connection state with timeout.
#[allow(dead_code)]
pub async fn wait_for_state(
    client: &TitanClient,
    expected: ConnectionState,
    timeout: Duration,
) -> bool {
    let mut receiver = client.state_receiver().await;
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let state = receiver.borrow_and_update().clone();
        if state == expected {
            return true;
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }

        tokio::select! {
            result = receiver.changed() => {
                if result.is_err() {
                    return false;
                }
            }
            () = tokio::time::sleep(remaining) => {
                return false;
            }
        }
    }
}

/// Initialize tracing for tests (call once per test module).
#[expect(dead_code, reason = "used by some test targets but not all")]
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("titan_rust_client=debug,test=debug")
        .with_test_writer()
        .try_init();
}
