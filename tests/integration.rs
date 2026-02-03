//! Integration tests against the real Titan API.
//!
//! These tests require TITAN_URL and TITAN_TOKEN in .env or environment.
//! Skipped by default — run with: `cargo test --test integration -- --ignored`
#![expect(clippy::expect_used)]

use titan_api_types::common::Pubkey;
use titan_api_types::ws::v1::{SwapMode, SwapParams, SwapQuoteRequest, TransactionParams};
use titan_rust_client::{TitanClient, TitanConfig};

const WSOL: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
const USDC: Pubkey = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const DUMMY_USER: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");

fn load_config() -> Option<TitanConfig> {
    let _ = dotenvy::dotenv();

    let url = std::env::var("TITAN_URL").ok()?;
    let token = std::env::var("TITAN_TOKEN").ok()?;

    if url.is_empty() || token.is_empty() {
        return None;
    }

    Some(TitanConfig::new(url, token).with_danger_accept_invalid_certs(true))
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("titan_rust_client=debug,integration=debug")
        .with_test_writer()
        .try_init();
}

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

// ========== One-shot API tests ==========

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_get_info() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");

    let client = TitanClient::new(config).await.expect("Failed to connect");

    let info = client.get_info().await.expect("Failed to get info");
    assert!(info.settings.connection.concurrent_streams > 0);
    assert!(info.protocol_version.major >= 1);

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_get_venues() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");

    let client = TitanClient::new(config).await.expect("Failed to connect");

    let venues = client.get_venues().await.expect("Failed to get venues");
    assert!(!venues.labels.is_empty());

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_list_providers() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");

    let client = TitanClient::new(config).await.expect("Failed to connect");

    let providers = client
        .list_providers()
        .await
        .expect("Failed to list providers");
    assert!(!providers.is_empty());

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_get_swap_price_simple() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");

    let client = TitanClient::new(config).await.expect("Failed to connect");

    let request = titan_api_types::ws::v1::SwapPriceRequest {
        input_mint: WSOL,
        output_mint: USDC,
        amount: 1_000_000,
        dexes: None,
        exclude_dexes: None,
    };

    let price = client
        .get_swap_price_simple(request)
        .await
        .expect("Failed to get swap price");

    assert_eq!(price.amount_in, 1_000_000);
    assert!(price.amount_out > 0);

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_concurrent_one_shot_requests() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");

    let client = TitanClient::new(config).await.expect("Failed to connect");

    let (info, venues, providers) = tokio::join!(
        client.get_info(),
        client.get_venues(),
        client.list_providers(),
    );

    assert!(info.is_ok());
    assert!(venues.is_ok());
    assert!(providers.is_ok());

    client.close().await.expect("Failed to close");
}

// ========== Streaming API tests ==========

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_open_stream_recv_and_stop() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");
    let client = TitanClient::new(config).await.expect("Failed to connect");

    assert_eq!(client.active_stream_count().await, 0);

    let mut stream = client
        .new_swap_quote_stream(make_quote_request(1_000_000))
        .await
        .expect("Failed to open stream");

    assert_eq!(client.active_stream_count().await, 1);

    let quotes = tokio::time::timeout(std::time::Duration::from_secs(10), stream.recv())
        .await
        .expect("Timed out waiting for quotes")
        .expect("Stream ended without sending quotes");

    assert_eq!(quotes.input_mint, WSOL);
    assert_eq!(quotes.output_mint, USDC);

    stream.stop().await.expect("Failed to stop stream");

    // Give the server a moment to process the stop
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(client.active_stream_count().await, 0);

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_stream_drop_releases_slot() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");
    let client = TitanClient::new(config).await.expect("Failed to connect");

    assert_eq!(client.active_stream_count().await, 0);

    {
        let mut stream = client
            .new_swap_quote_stream(make_quote_request(1_000_000))
            .await
            .expect("Failed to open stream");

        assert_eq!(client.active_stream_count().await, 1);

        // Receive at least one quote to confirm stream is working
        let _quotes = tokio::time::timeout(std::time::Duration::from_secs(10), stream.recv())
            .await
            .expect("Timed out waiting for quotes")
            .expect("Stream ended without sending quotes");

        // stream dropped here
    }

    // Drop spawns a tokio task — give it time to send StopStream and decrement
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(client.active_stream_count().await, 0);

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_multiple_streams_slot_accounting() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");
    let client = TitanClient::new(config).await.expect("Failed to connect");

    let max_streams = client
        .get_info()
        .await
        .expect("Failed to get info")
        .settings
        .connection
        .concurrent_streams;

    // Open 2 streams (staying safely under the limit)
    let stream_count = 2.min(max_streams);

    let mut streams = Vec::new();
    for i in 0..stream_count {
        let amount = 1_000_000 + u64::from(i) * 100_000;
        let stream = client
            .new_swap_quote_stream(make_quote_request(amount))
            .await
            .expect("Failed to open stream");
        streams.push(stream);
    }

    assert_eq!(client.active_stream_count().await, stream_count);

    // Receive one quote from each to confirm they're alive
    for stream in &mut streams {
        let _quotes = tokio::time::timeout(std::time::Duration::from_secs(10), stream.recv())
            .await
            .expect("Timed out waiting for quotes")
            .expect("Stream ended unexpectedly");
    }

    // Stop streams one at a time, verifying count decrements
    for (i, stream) in streams.iter_mut().enumerate() {
        stream.stop().await.expect("Failed to stop stream");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let expected = stream_count - u32::try_from(i).expect("too many streams") - 1;
        assert_eq!(
            client.active_stream_count().await,
            expected,
            "active_count mismatch after stopping stream {i}"
        );
    }

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_open_stop_reopen_no_leak() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");
    let client = TitanClient::new(config).await.expect("Failed to connect");

    // Cycle: open → recv → stop, repeated 3 times.
    // Before our fix, the slot would desync and the 2nd or 3rd cycle could fail
    // with "Too many concurrent streams" if slots leaked.
    for cycle in 0..3 {
        assert_eq!(
            client.active_stream_count().await,
            0,
            "Leaked slot before cycle {cycle}"
        );

        let mut stream = client
            .new_swap_quote_stream(make_quote_request(1_000_000))
            .await
            .expect("Failed to open stream");

        assert_eq!(client.active_stream_count().await, 1);

        let _quotes = tokio::time::timeout(std::time::Duration::from_secs(10), stream.recv())
            .await
            .expect("Timed out")
            .expect("Stream ended");

        stream.stop().await.expect("Failed to stop");

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    assert_eq!(client.active_stream_count().await, 0);
    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_stream_double_stop_is_safe() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");
    let client = TitanClient::new(config).await.expect("Failed to connect");

    let mut stream = client
        .new_swap_quote_stream(make_quote_request(1_000_000))
        .await
        .expect("Failed to open stream");

    let _quotes = tokio::time::timeout(std::time::Duration::from_secs(10), stream.recv())
        .await
        .expect("Timed out")
        .expect("Stream ended");

    // First stop
    stream.stop().await.expect("First stop failed");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(client.active_stream_count().await, 0);

    // Second stop — should be a no-op, not underflow active_count
    stream.stop().await.expect("Second stop failed");
    assert_eq!(client.active_stream_count().await, 0);

    client.close().await.expect("Failed to close");
}

// ========== One-shot wrapper (stream-based) tests ==========

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_get_swap_price_via_stream() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");
    let client = TitanClient::new(config).await.expect("Failed to connect");

    let quotes = client
        .get_swap_price(make_quote_request(1_000_000))
        .await
        .expect("Failed to get swap price via stream");

    assert_eq!(quotes.input_mint, WSOL);
    assert_eq!(quotes.output_mint, USDC);

    // Stream should have been auto-stopped
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(client.active_stream_count().await, 0);

    client.close().await.expect("Failed to close");
}

#[tokio::test]
#[ignore = "requires TITAN_URL and TITAN_TOKEN"]
async fn real_get_swap_price_slot_release() {
    init_tracing();
    let config = load_config().expect("TITAN_URL and TITAN_TOKEN must be set");
    let client = TitanClient::new(config).await.expect("Failed to connect");

    for cycle in 0..3 {
        assert_eq!(
            client.active_stream_count().await,
            0,
            "Leaked slot before cycle {cycle}"
        );

        let _quotes = client
            .get_swap_price(make_quote_request(1_000_000))
            .await
            .expect("Failed to get swap price");

        // Give the stream time to fully clean up
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    assert_eq!(client.active_stream_count().await, 0);
    client.close().await.expect("Failed to close");
}
