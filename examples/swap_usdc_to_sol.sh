#!/bin/bash
# Example: Swap USDC to SOL
#
# Prerequisites:
#   1. Set TITAN_TOKEN environment variable or in .env file
#   2. Have a Solana keypair JSON file
#
# Usage:
#   ./examples/swap_usdc_to_sol.sh <keypair_path> <amount_in_usdc>
#
# Example:
#   ./examples/swap_usdc_to_sol.sh ~/.config/solana/id.json 10

set -e

# Load .env if present
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

# Check required environment
if [ -z "$TITAN_TOKEN" ]; then
    echo "Error: TITAN_TOKEN environment variable not set"
    exit 1
fi

KEYPAIR_PATH="${1:-$KEYPAIR_PATH}"
AMOUNT_USDC="${2:-1}"

if [ -z "$KEYPAIR_PATH" ]; then
    echo "Error: Keypair path required"
    echo "Usage: $0 <keypair_path> [amount_in_usdc]"
    exit 1
fi

# Convert USDC to base units (1 USDC = 1,000,000 units - 6 decimals)
AMOUNT_UNITS=$(echo "$AMOUNT_USDC * 1000000" | bc | cut -d'.' -f1)

echo "Swapping $AMOUNT_USDC USDC ($AMOUNT_UNITS units) to SOL..."

cargo run --features cli --bin titan-cli -- swap \
    --keypair "$KEYPAIR_PATH" \
    --slippage-bps 50 \
    USDC SOL "$AMOUNT_UNITS"
