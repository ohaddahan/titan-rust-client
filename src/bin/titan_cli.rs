//! Titan CLI test binary.
//!
//! Run with: cargo run --bin titan-cli --features cli -- --help

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("CLI feature not enabled. Run with: cargo run --features cli --bin titan-cli");
    std::process::exit(1);
}

#[cfg(feature = "cli")]
fn main() {
    cli::run();
}

#[cfg(feature = "cli")]
mod cli {
    use clap::{Parser, Subcommand};
    use titan_rust_client::{TitanClient, TitanConfig};

    /// Titan Exchange CLI client for testing and debugging.
    #[derive(Parser)]
    #[command(name = "titan-cli")]
    #[command(about = "Titan Exchange WebSocket API client")]
    struct Cli {
        /// WebSocket URL
        #[arg(
            long,
            env = "TITAN_URL",
            default_value = "wss://api.titan.ag/api/v1/ws"
        )]
        url: String,

        /// Authentication token
        #[arg(long, env = "TITAN_TOKEN")]
        token: String,

        #[command(subcommand)]
        command: Commands,
    }

    #[derive(Subcommand)]
    enum Commands {
        /// Get server info and connection limits
        Info,

        /// Get available trading venues
        Venues,

        /// List available liquidity providers
        Providers,

        /// Get a point-in-time swap price
        Price {
            /// Input token mint (e.g., SOL, USDC, or mint address)
            #[arg(default_value = "So11111111111111111111111111111111111111112")]
            input_mint: String,

            /// Output token mint (e.g., SOL, USDC, or mint address)
            #[arg(default_value = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")]
            output_mint: String,

            /// Amount in base units (lamports for SOL)
            #[arg(default_value = "1000000000")]
            amount: u64,
        },

        /// Stream swap quotes continuously
        Stream {
            /// Input token mint
            #[arg(default_value = "So11111111111111111111111111111111111111112")]
            input_mint: String,

            /// Output token mint
            #[arg(default_value = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")]
            output_mint: String,

            /// Amount in base units
            #[arg(default_value = "1000000000")]
            amount: u64,

            /// User public key for transaction generation
            #[arg(long)]
            user: Option<String>,
        },

        /// Watch connection state changes
        Watch,
    }

    pub fn run() {
        // Load .env file if present
        let _ = dotenvy::dotenv();

        // Initialize tracing
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::INFO.into()),
            )
            .init();

        let cli = Cli::parse();

        // Build runtime and run
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        rt.block_on(run_command(cli));
    }

    async fn run_command(cli: Cli) {
        let config = TitanConfig::new(&cli.url, &cli.token);

        println!("Connecting to {}...", cli.url);

        let client = match TitanClient::new(config).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to connect: {}", e);
                std::process::exit(1);
            }
        };

        println!("Connected!\n");

        let result = match cli.command {
            Commands::Info => cmd_info(&client).await,
            Commands::Venues => cmd_venues(&client).await,
            Commands::Providers => cmd_providers(&client).await,
            Commands::Price {
                input_mint,
                output_mint,
                amount,
            } => cmd_price(&client, &input_mint, &output_mint, amount).await,
            Commands::Stream {
                input_mint,
                output_mint,
                amount,
                user,
            } => cmd_stream(&client, &input_mint, &output_mint, amount, user).await,
            Commands::Watch => cmd_watch(&client).await,
        };

        if let Err(e) = result {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }

        let _ = client.close().await;
    }

    async fn cmd_info(client: &TitanClient) -> Result<(), Box<dyn std::error::Error>> {
        let info = client.get_info().await?;
        println!("Server Info:");
        println!(
            "  Protocol Version: {}.{}.{}",
            info.protocol_version.major, info.protocol_version.minor, info.protocol_version.patch
        );
        println!(
            "  Max Concurrent Streams: {}",
            info.settings.connection.concurrent_streams
        );
        println!(
            "  Swap Settings: slippage_bps {}..{} (default {})",
            info.settings.swap.slippage_bps.min,
            info.settings.swap.slippage_bps.max,
            info.settings.swap.slippage_bps.default
        );
        Ok(())
    }

    async fn cmd_venues(client: &TitanClient) -> Result<(), Box<dyn std::error::Error>> {
        let venues = client.get_venues().await?;
        println!("Available Venues ({}):", venues.labels.len());
        for (i, label) in venues.labels.iter().enumerate() {
            if let Some(ref program_ids) = venues.program_ids {
                if let Some(pid) = program_ids.get(i) {
                    println!("  - {} ({})", label, pid);
                } else {
                    println!("  - {}", label);
                }
            } else {
                println!("  - {}", label);
            }
        }
        Ok(())
    }

    async fn cmd_providers(client: &TitanClient) -> Result<(), Box<dyn std::error::Error>> {
        let providers = client.list_providers().await?;
        println!("Available Providers ({}):", providers.len());
        for provider in &providers {
            println!("  - {} ({:?})", provider.name, provider.kind);
        }
        Ok(())
    }

    async fn cmd_price(
        client: &TitanClient,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use titan_rust_client::types::SwapPriceRequest;

        let input = parse_mint(input_mint)?;
        let output = parse_mint(output_mint)?;

        let request = SwapPriceRequest {
            input_mint: input,
            output_mint: output,
            amount,
            dexes: None,
            exclude_dexes: None,
        };

        println!(
            "Getting price for {} {} -> {}...",
            amount, input_mint, output_mint
        );

        let price = client.get_swap_price(request).await?;
        println!("\nSwap Price:");
        println!("  In Amount: {}", price.amount_in);
        println!("  Out Amount: {}", price.amount_out);

        Ok(())
    }

    async fn cmd_stream(
        client: &TitanClient,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        user: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use titan_rust_client::types::{SwapMode, SwapParams, SwapQuoteRequest, TransactionParams};

        let input = parse_mint(input_mint)?;
        let output = parse_mint(output_mint)?;

        // Default to a dummy pubkey if not specified
        let user_pubkey = match user {
            Some(ref u) => parse_mint(u)?,
            None => parse_mint("11111111111111111111111111111111")?, // System program as placeholder
        };

        let request = SwapQuoteRequest {
            swap: SwapParams {
                input_mint: input,
                output_mint: output,
                amount,
                swap_mode: Some(SwapMode::ExactIn),
                slippage_bps: Some(50),
                ..Default::default()
            },
            transaction: TransactionParams {
                user_public_key: user_pubkey,
                ..Default::default()
            },
            update: None,
        };

        println!(
            "Streaming quotes for {} {} -> {} (Ctrl+C to stop)...\n",
            amount, input_mint, output_mint
        );

        let mut stream = client.new_swap_quote_stream(request).await?;
        let mut count = 0u32;

        // Handle Ctrl+C
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            let _ = tx.send(()).await;
        });

        loop {
            tokio::select! {
                quote = stream.recv() => {
                    match quote {
                        Some(quotes) => {
                            count += 1;
                            println!("Quote #{} ({} routes):", count, quotes.quotes.len());
                            for (id, route) in &quotes.quotes {
                                println!(
                                    "  Route {}: {} -> {} (slippage: {} bps)",
                                    id, route.in_amount, route.out_amount, route.slippage_bps
                                );
                                if !route.steps.is_empty() {
                                    print!("    Path: ");
                                    for (i, step) in route.steps.iter().enumerate() {
                                        if i > 0 {
                                            print!(" -> ");
                                        }
                                        print!("{}", step.label);
                                    }
                                    println!();
                                }
                            }
                            println!();
                        }
                        None => {
                            println!("Stream ended");
                            break;
                        }
                    }
                }
                _ = rx.recv() => {
                    println!("\nStopping stream...");
                    stream.stop().await?;
                    break;
                }
            }
        }

        println!("Received {} quotes total", count);
        Ok(())
    }

    async fn cmd_watch(client: &TitanClient) -> Result<(), Box<dyn std::error::Error>> {
        println!("Watching connection state (Ctrl+C to stop)...\n");

        let mut receiver = client.state_receiver().await;

        // Handle Ctrl+C
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            let _ = tx.send(()).await;
        });

        // Print initial state
        println!("Current state: {}", *receiver.borrow());

        loop {
            tokio::select! {
                result = receiver.changed() => {
                    if result.is_err() {
                        println!("Connection closed");
                        break;
                    }
                    println!("State changed: {}", *receiver.borrow());
                }
                _ = rx.recv() => {
                    println!("\nStopping...");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Parse a mint string - either a known symbol or a base58 pubkey
    fn parse_mint(s: &str) -> Result<titan_rust_client::types::Pubkey, Box<dyn std::error::Error>> {
        // Known token symbols
        let pubkey_str = match s.to_uppercase().as_str() {
            "SOL" | "WSOL" => "So11111111111111111111111111111111111111112",
            "USDC" => "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "USDT" => "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
            _ => s,
        };

        // Parse as base58
        let bytes: [u8; 32] = bs58::decode(pubkey_str)
            .into_vec()?
            .try_into()
            .map_err(|_| "Invalid pubkey length")?;

        Ok(titan_rust_client::types::Pubkey::from(bytes))
    }
}
