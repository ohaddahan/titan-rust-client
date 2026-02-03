//! Connection and state management tests.
#![expect(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};
use titan_api_types::common::Pubkey;
use titan_api_types::ws::v1::{SwapMode, SwapParams, SwapQuoteRequest, TransactionParams};
use titan_rust_client::{ConnectionState, TitanClient, TitanClientError};

use crate::common::{
    init_tracing,
    mock_server::{mock_stream_data, mock_swap_quotes},
    test_config, MockTitanServer,
};

const WSOL: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
const USDC: Pubkey = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const DUMMY_USER: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");

fn make_quote_request(amount: u64) -> SwapQuoteRequest {
    SwapQuoteRequest {
        swap: SwapParams {
            input_mint: WSOL,
            output_mint: USDC,
            amount,
            swap_mode: Some(SwapMode::ExactIn),
            slippage_bps: Some(50),
            ..Default::default()
        },
        transaction: TransactionParams {
            user_public_key: DUMMY_USER,
            ..Default::default()
        },
        update: None,
    }
}

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
async fn test_close_shuts_down_connection() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let config = test_config(&server.url());

    let client = TitanClient::new(config).await.expect("Failed to connect");
    assert_eq!(server.connected_clients(), 1);

    client.close().await.expect("Failed to close");

    let deadline = Instant::now() + Duration::from_secs(2);
    while server.connected_clients() > 0 {
        if Instant::now() >= deadline {
            panic!("Server still has connected clients after close()");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

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
async fn test_pending_request_fails_on_disconnect() {
    init_tracing();

    let server = MockTitanServer::start().await;
    server.set_disconnect_after(Some(2)).await;

    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let result = tokio::time::timeout(Duration::from_secs(2), client.get_info())
        .await
        .expect("Timed out waiting for get_info");

    let err = result.expect_err("Expected disconnect error");
    assert!(
        matches!(err, TitanClientError::ConnectionClosed { .. }),
        "Expected ConnectionClosed, got {err:?}"
    );

    client.close().await.expect("Failed to close");
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

#[tokio::test]
async fn test_resumption_with_interleaved_frames() {
    init_tracing();

    let server = MockTitanServer::start().await;
    server.set_disconnect_after(Some(3)).await;

    let interleaved = vec![mock_stream_data(
        999,
        1,
        mock_swap_quotes(WSOL, USDC, 1_000_000, 1_500_000),
    )];
    server.set_interleave_on_new_stream(interleaved, true).await;

    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let mut stream = client
        .new_swap_quote_stream(make_quote_request(1_000_000))
        .await
        .expect("Failed to open stream");

    let original_id = stream.stream_id();

    let _ = client.get_info().await;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if stream.effective_stream_id() != original_id {
            break;
        }
        if Instant::now() >= deadline {
            panic!("Stream was not resumed with a new ID");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(client.active_stream_count().await, 1);

    stream.stop().await.expect("Failed to stop stream");
    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_resumption_failure_releases_slot() {
    init_tracing();

    let server = MockTitanServer::start().await;
    server.set_disconnect_after(Some(3)).await;

    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let mut stream = client
        .new_swap_quote_stream(make_quote_request(1_000_000))
        .await
        .expect("Failed to open stream");

    assert_eq!(client.active_stream_count().await, 1);

    server.set_reject_new_stream(true, true).await;

    let _ = client.get_info().await;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if client.active_stream_count().await == 0 {
            break;
        }
        if Instant::now() >= deadline {
            panic!("Stream slot was not released after resumption failure");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let _ = stream.stop().await;
    client.close().await.expect("Failed to close");
    server.stop().await;
}
