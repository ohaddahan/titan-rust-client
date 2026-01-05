//! Streaming API tests.

mod common;

use titan_api_types::common::Pubkey;
use titan_api_types::ws::v1::{SwapMode, SwapParams, SwapQuoteRequest, TransactionParams};
use titan_rust_client::TitanClient;

use crate::common::{init_tracing, test_config, MockTitanServer};

const WSOL: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
const USDC: Pubkey = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

fn make_quote_request(input: Pubkey, output: Pubkey, amount: u64) -> SwapQuoteRequest {
    SwapQuoteRequest {
        swap: SwapParams {
            input_mint: input,
            output_mint: output,
            amount,
            swap_mode: Some(SwapMode::ExactIn),
            slippage_bps: Some(50),
            ..Default::default()
        },
        transaction: TransactionParams {
            user_public_key: Pubkey::default(),
            ..Default::default()
        },
        update: None,
    }
}

#[tokio::test]
async fn test_start_stream() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let request = make_quote_request(WSOL, USDC, 1_000_000_000);

    let mut stream = client
        .new_swap_quote_stream(request)
        .await
        .expect("Failed to start stream");

    // Verify stream was started
    assert_eq!(client.active_stream_count().await, 1);

    // Stop the stream
    stream.stop().await.expect("Failed to stop stream");

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_stream_stop() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let request = make_quote_request(WSOL, USDC, 1_000_000_000);

    let mut stream = client
        .new_swap_quote_stream(request)
        .await
        .expect("Failed to start stream");

    assert_eq!(client.active_stream_count().await, 1);

    // Stop the stream explicitly
    stream.stop().await.expect("Failed to stop stream");

    // Give time for cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_multiple_streams() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let request1 = make_quote_request(WSOL, USDC, 1_000_000_000);
    let request2 = make_quote_request(USDC, WSOL, 100_000_000);

    let mut stream1 = client
        .new_swap_quote_stream(request1)
        .await
        .expect("Failed to start stream 1");

    let mut stream2 = client
        .new_swap_quote_stream(request2)
        .await
        .expect("Failed to start stream 2");

    assert_eq!(client.active_stream_count().await, 2);

    stream1.stop().await.expect("Failed to stop stream 1");
    stream2.stop().await.expect("Failed to stop stream 2");

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_stream_id_uniqueness() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let request = make_quote_request(WSOL, USDC, 1_000_000_000);

    let mut stream1 = client
        .new_swap_quote_stream(request.clone())
        .await
        .expect("Failed to start stream 1");

    let mut stream2 = client
        .new_swap_quote_stream(request)
        .await
        .expect("Failed to start stream 2");

    // Each stream should have a unique ID
    assert_ne!(stream1.stream_id(), stream2.stream_id());

    stream1.stop().await.expect("Failed to stop stream 1");
    stream2.stop().await.expect("Failed to stop stream 2");

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_stream_cleanup_on_close() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let request = make_quote_request(WSOL, USDC, 1_000_000_000);

    let _stream = client
        .new_swap_quote_stream(request)
        .await
        .expect("Failed to start stream");

    assert_eq!(client.active_stream_count().await, 1);

    // Close should clean up all streams
    client.close().await.expect("Failed to close");

    server.stop().await;
}

#[tokio::test]
async fn test_stream_queue_metrics() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    // Initially no streams
    assert_eq!(client.active_stream_count().await, 0);
    assert_eq!(client.queued_stream_count().await, 0);

    let request = make_quote_request(WSOL, USDC, 1_000_000_000);
    let mut stream = client
        .new_swap_quote_stream(request)
        .await
        .expect("Failed to start stream");

    assert_eq!(client.active_stream_count().await, 1);

    stream.stop().await.expect("Failed to stop stream");

    client.close().await.expect("Failed to close");
    server.stop().await;
}
