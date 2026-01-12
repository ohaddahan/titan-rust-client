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
    source .env
fi

# Check required environment
if [ -z "$TITAN_TOKEN" ]; then
    echo "Error: TITAN_TOKEN environment variable not set"
    exit 1
fi


cargo run \
--features cli \
--bin titan-cli \
-- \
--danger-accept-invalid-certs \
swap \
--keypair "/Users/ohaddahan/.config/solana/id.json" \
--slippage-bps 50 \
USDT SOL "1000000"
