//! Integration tests against the real Titan API.
//!
//! These tests require TITAN_URL and TITAN_TOKEN in .env or environment.
//! Skipped by default — run with: `cargo test --test integration -- --ignored`
#![expect(clippy::expect_used)]

use titan_api_types::common::Pubkey;
use titan_rust_client::{TitanClient, TitanConfig};

const WSOL: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
const USDC: Pubkey = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

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
async fn real_get_swap_price() {
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
        .get_swap_price(request)
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
