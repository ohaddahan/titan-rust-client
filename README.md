# titan-rust-client

A Rust client library for the [Titan Exchange](https://titan.exchange) WebSocket API, enabling real-time swap quote streaming and execution on Solana.

[![Crates.io](https://img.shields.io/crates/v/titan-rust-client.svg)](https://crates.io/crates/titan-rust-client)
[![Documentation](https://docs.rs/titan-rust-client/badge.svg)](https://docs.rs/titan-rust-client)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Features

- **Real-time Quote Streaming** - Stream live swap quotes with automatic updates
- **One-shot Queries** - Get server info, venues, providers, and instant price checks
- **Full Swap Execution** - Build, sign, and submit swap transactions to Solana
- **Auto-reconnect** - Automatic reconnection with exponential backoff
- **Stream Resumption** - Resume quote streams after reconnection
- **Connection State Observable** - Monitor connection state changes
- **TLS Support** - Secure WebSocket connections via rustls

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
titan-rust-client = "0.1"
```

For the CLI binary:

```toml
[dependencies]
titan-rust-client = { version = "0.1", features = ["cli"] }
```

## Quick Start

### Library Usage

```rust
use titan_rust_client::{TitanClient, TitanConfig};
use titan_rust_client::types::{SwapMode, SwapParams, SwapQuoteRequest, TransactionParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize rustls crypto provider (required once at startup)
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Connect to Titan
    let config = TitanConfig::new(
        "wss://us1.api.demo.titan.exchange/api/v1/ws",
        "your-api-token",
    );
    let client = TitanClient::new(config).await?;

    // Get server info
    let info = client.get_info().await?;
    println!("Protocol: {}.{}.{}",
        info.protocol_version.major,
        info.protocol_version.minor,
        info.protocol_version.patch
    );

    // Get available venues
    let venues = client.get_venues().await?;
    println!("Available venues: {}", venues.labels.len());

    // Get a price quote
    let price = client.get_swap_price(SwapPriceRequest {
        input_mint: sol_mint(),
        output_mint: usdc_mint(),
        amount: 1_000_000_000, // 1 SOL in lamports
        dexes: None,
        exclude_dexes: None,
    }).await?;
    println!("1 SOL = {} USDC", price.amount_out as f64 / 1_000_000.0);

    // Stream live quotes
    let request = SwapQuoteRequest {
        swap: SwapParams {
            input_mint: sol_mint(),
            output_mint: usdc_mint(),
            amount: 1_000_000_000,
            swap_mode: Some(SwapMode::ExactIn),
            slippage_bps: Some(50),
            ..Default::default()
        },
        transaction: TransactionParams {
            user_public_key: your_pubkey(),
            ..Default::default()
        },
        update: None,
    };

    let mut stream = client.new_swap_quote_stream(request).await?;

    // Receive quotes
    while let Some(quotes) = stream.recv().await {
        for (provider, route) in &quotes.quotes {
            println!("{}: {} -> {}", provider, route.in_amount, route.out_amount);
        }
    }

    client.close().await?;
    Ok(())
}
```

### Executing a Swap

```rust
use titan_rust_client::TitanInstructions;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    signature::Keypair,
    signer::Signer,
    transaction::VersionedTransaction,
    message::{v0::Message, VersionedMessage},
    compute_budget::ComputeBudgetInstruction,
};

// Get a quote with instructions
let mut stream = client.new_swap_quote_stream(request).await?;
let quotes = stream.recv().await.unwrap();
stream.stop().await?;

// Pick the best route
let (_, best_route) = quotes.quotes
    .iter()
    .max_by_key(|(_, r)| r.out_amount)
    .unwrap();

// Prepare instructions (fetches ALTs from chain)
let rpc = RpcClient::new("https://api.mainnet-beta.solana.com".to_string());
let prepared = TitanInstructions::prepare_instructions(best_route, &rpc).await?;

// Build transaction
let mut instructions = vec![];
if let Some(units) = prepared.compute_units_safe {
    instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(units as u32));
}
instructions.extend(prepared.instructions);

let blockhash = rpc.get_latest_blockhash().await?;
let message = Message::try_compile(
    &keypair.pubkey(),
    &instructions,
    &prepared.address_lookup_table_accounts,
    blockhash,
)?;

let transaction = VersionedTransaction::try_new(
    VersionedMessage::V0(message),
    &[&keypair],
)?;

// Send transaction
let signature = rpc.send_and_confirm_transaction(&transaction).await?;
println!("Swap executed: {}", signature);
```

## CLI Usage

The crate includes a CLI for testing and quick swaps.

### Installation

```bash
cargo install titan-rust-client --features cli
```

### Configuration

Create a `.env` file or set environment variables:

```env
TITAN_TOKEN=your-api-token
TITAN_URL=wss://us1.api.demo.titan.exchange/api/v1/ws
KEYPAIR_PATH=/path/to/keypair.json
SOLANA_RPC_URL=https://api.mainnet-beta.solana.com
```

### Commands

```bash
# Get server info
titan-cli info

# List available venues
titan-cli venues

# List liquidity providers
titan-cli providers

# Get swap price
titan-cli price SOL USDC 1000000000

# Stream live quotes
titan-cli stream SOL USDC 1000000000

# Execute a swap (interactive)
titan-cli swap --keypair ~/.config/solana/id.json SOL USDC 100000000

# Execute a swap (automated, no confirmation)
titan-cli swap --keypair ~/.config/solana/id.json --yes SOL USDC 100000000

# Watch connection state
titan-cli watch
```

### Token Shortcuts

The CLI supports these token shortcuts:
- `SOL` / `WSOL` - Wrapped SOL
- `USDC` - USD Coin
- `USDT` - Tether

Or use full mint addresses:
```bash
titan-cli price So11111111111111111111111111111111111111112 EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v 1000000000
```

## API Reference

### TitanClient

| Method | Description |
|--------|-------------|
| `new(config)` | Create and connect a new client |
| `get_info()` | Get server info and settings |
| `get_venues()` | Get available trading venues |
| `list_providers()` | List liquidity providers |
| `get_swap_price(request)` | Get instant swap price |
| `new_swap_quote_stream(request)` | Start streaming quotes |
| `state_receiver()` | Get connection state observable |
| `close()` | Gracefully disconnect |

### TitanInstructions

| Method | Description |
|--------|-------------|
| `prepare_instructions(route, rpc)` | Convert route to Solana instructions |
| `fetch_address_lookup_tables(addresses, rpc)` | Fetch ALT accounts |
| `convert_instructions(instructions)` | Convert Titan instructions to Solana SDK |

### Connection States

```rust
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Failed { error: String },
}
```

## Features

| Feature | Description |
|---------|-------------|
| `default` | Core library only |
| `cli` | Include CLI binary and dependencies |

## Requirements

- Rust 1.75+
- Titan API token (get from [titan.exchange](https://titan.exchange))

## License

MIT License - see [LICENSE](LICENSE) for details.

## Related

- [Titan Exchange](https://titan.exchange) - DEX aggregator for Solana
- [titan-api-types](https://crates.io/crates/titan-api-types) - API type definitions
- [titan-api-codec](https://crates.io/crates/titan-api-codec) - Message encoding/decoding
