//! One-shot API method tests.

mod common;

use titan_api_types::common::Pubkey;
use titan_api_types::ws::v1::SwapPriceRequest;
use titan_rust_client::TitanClient;

use crate::common::{init_tracing, test_config, MockTitanServer};

const WSOL: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
const USDC: Pubkey = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

#[tokio::test]
async fn test_get_info() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let info = client.get_info().await.expect("Failed to get info");

    assert_eq!(info.protocol_version.major, 1);
    assert_eq!(info.protocol_version.minor, 2);
    assert_eq!(info.settings.connection.concurrent_streams, 10);
    assert_eq!(info.settings.swap.slippage_bps.default, 50);

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_get_venues() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let venues = client.get_venues().await.expect("Failed to get venues");

    assert_eq!(venues.labels.len(), 3);
    assert!(venues.labels.contains(&"Raydium".to_string()));
    assert!(venues.labels.contains(&"Orca".to_string()));
    assert!(venues.labels.contains(&"Meteora".to_string()));
    assert!(venues.program_ids.is_some());

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_list_providers() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let providers = client
        .list_providers()
        .await
        .expect("Failed to list providers");

    assert_eq!(providers.len(), 2);
    assert!(providers.iter().any(|p| p.id == "jupiter"));
    assert!(providers.iter().any(|p| p.id == "titan"));

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_get_swap_price() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    let request = SwapPriceRequest {
        input_mint: WSOL,
        output_mint: USDC,
        amount: 1_000_000_000, // 1 SOL
        dexes: None,
        exclude_dexes: None,
    };

    let price = client
        .get_swap_price(request)
        .await
        .expect("Failed to get swap price");

    assert_eq!(price.amount_in, 1_000_000_000);
    assert_eq!(price.amount_out, 1_000_000_000 * 150); // Mock returns 150x
    assert_eq!(price.input_mint, WSOL);
    assert_eq!(price.output_mint, USDC);

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_multiple_requests() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    // Make multiple requests in sequence
    let info = client.get_info().await.expect("Failed to get info");
    assert_eq!(info.protocol_version.major, 1);

    let venues = client.get_venues().await.expect("Failed to get venues");
    assert!(!venues.labels.is_empty());

    let providers = client
        .list_providers()
        .await
        .expect("Failed to list providers");
    assert!(!providers.is_empty());

    client.close().await.expect("Failed to close");
    server.stop().await;
}

#[tokio::test]
async fn test_concurrent_requests() {
    init_tracing();

    let server = MockTitanServer::start().await;
    let client = TitanClient::new(test_config(&server.url()))
        .await
        .expect("Failed to connect");

    // Make concurrent requests
    let (info, venues, providers) = tokio::join!(
        client.get_info(),
        client.get_venues(),
        client.list_providers()
    );

    assert!(info.is_ok());
    assert!(venues.is_ok());
    assert!(providers.is_ok());

    client.close().await.expect("Failed to close");
    server.stop().await;
}
