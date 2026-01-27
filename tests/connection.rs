//! Connection and state management tests.
#![expect(clippy::expect_used, clippy::unwrap_used)]

mod common;

use titan_rust_client::{ConnectionState, TitanClient};

use crate::common::{init_tracing, test_config, MockTitanServer};

#[tokio::test]
async fn test_connect_to_mock_server() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let config = test_config(&server.url());

    let client = TitanClient::new(config).await.expect("Failed to connect");

    assert!(client.is_connected().await);
    assert_eq!(server.connected_clients(), 1);

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_connection_state_observable() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let config = test_config(&server.url());

    let client = TitanClient::new(config).await.expect("Failed to connect");

    // Check initial state is Connected
    let state = client.state().await;
    assert!(state.is_connected());

    // Get receiver and verify it works
    let receiver = client.state_receiver().await;
    assert!(receiver.borrow().is_connected());

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_graceful_shutdown() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let config = test_config(&server.url());

    let client = TitanClient::new(config).await.expect("Failed to connect");

    assert!(!client.is_closed().await);

    client.close().await.expect("Failed to close");

    assert!(client.is_closed().await);

    server.stop().await;
}

#[tokio::test]
async fn test_wait_for_connected() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let config = test_config(&server.url());

    let client = TitanClient::new(config).await.expect("Failed to connect");

    // Should return immediately since already connected
    client
        .wait_for_connected()
        .await
        .expect("Should be connected");

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_multiple_clients() {
    init_tracing();

    let server = MockTitanServer::start().await;

    let client1 = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect client 1");

    let client2 = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect client 2");

    assert!(client1.is_connected().await);
    assert!(client2.is_connected().await);
    assert_eq!(server.connected_clients(), 2);

    client1.close().await.expect("Failed to close client 1");
    client2.close().await.expect("Failed to close client 2");

    server.stop().await;
}

#[tokio::test]
async fn test_connection_state_display() {
    let connected = ConnectionState::Connected;
    assert_eq!(format!("{}", connected), "Connected");

    let reconnecting = ConnectionState::Reconnecting { attempt: 3 };
    assert_eq!(format!("{}", reconnecting), "Reconnecting (attempt 3)");

    let disconnected = ConnectionState::Disconnected {
        reason: "Server closed".to_string(),
    };
    assert_eq!(format!("{}", disconnected), "Disconnected: Server closed");
}

#[tokio::test]
async fn test_connection_state_helpers() {
    let connected = ConnectionState::Connected;
    assert!(connected.is_connected());
    assert!(!connected.is_reconnecting());
    assert!(!connected.is_disconnected());
    assert_eq!(connected.reconnect_attempt(), None);
    assert_eq!(connected.disconnect_reason(), None);

    let reconnecting = ConnectionState::Reconnecting { attempt: 5 };
    assert!(!reconnecting.is_connected());
    assert!(reconnecting.is_reconnecting());
    assert!(!reconnecting.is_disconnected());
    assert_eq!(reconnecting.reconnect_attempt(), Some(5));
    assert_eq!(reconnecting.disconnect_reason(), None);

    let disconnected = ConnectionState::Disconnected {
        reason: "Test reason".to_string(),
    };
    assert!(!disconnected.is_connected());
    assert!(!disconnected.is_reconnecting());
    assert!(disconnected.is_disconnected());
    assert_eq!(disconnected.reconnect_attempt(), None);
    assert_eq!(disconnected.disconnect_reason(), Some("Test reason"));
}
