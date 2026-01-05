#!/bin/bash
# Example: Swap SOL to USDC
#
# Prerequisites:
#   1. Set TITAN_TOKEN environment variable or in .env file
#   2. Have a Solana keypair JSON file
#
# Usage:
#   ./examples/swap_sol_to_usdc.sh <keypair_path> <amount_in_sol>
#
# Example:
#   ./examples/swap_sol_to_usdc.sh ~/.config/solana/id.json 0.1
set -e
# Load .env if present
if [ -f .env ]; then
  #export $(cat .env | grep -v '^#' | xargs)
  source .env
fi
# Check required environment
if [ -z "$TITAN_TOKEN" ]; then
    echo "Error: TITAN_TOKEN environment variable not set"
    exit 1
fi
AMOUNT_LAMPORTS="10000000"
echo "Swapping \$SOL ($AMOUNT_LAMPORTS lamports) to USDC..."
cargo run --features cli --bin titan-cli -- swap \
    --keypair "/Users/ohaddahan/.config/solana/id.json" \
    --slippage-bps 50 \
    SOL USDC "$AMOUNT_LAMPORTS"