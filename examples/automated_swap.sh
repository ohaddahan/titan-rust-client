#!/bin/bash
# Example: Automated swap without confirmation prompt
#
# Use --yes flag to skip the confirmation prompt for automated scripts.
# WARNING: This will execute the swap immediately!
#
# Prerequisites:
#   1. Set environment variables in .env file
#   2. Have a Solana keypair JSON file
#
# Usage:
#   ./examples/automated_swap.sh

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

if [ -z "$KEYPAIR_PATH" ]; then
    echo "Error: KEYPAIR_PATH environment variable not set"
    exit 1
fi

# Configuration
INPUT_MINT="SOL"
OUTPUT_MINT="USDC"
AMOUNT="100000000"  # 0.1 SOL in lamports
SLIPPAGE_BPS="100"  # 1% slippage for volatile conditions

echo "Executing automated swap..."
echo "  Input:    $INPUT_MINT"
echo "  Output:   $OUTPUT_MINT"
echo "  Amount:   $AMOUNT"
echo "  Slippage: ${SLIPPAGE_BPS}bps"

# Execute with --yes to skip confirmation
cargo run --features cli --bin titan-cli -- swap \
    --keypair "$KEYPAIR_PATH" \
    --slippage-bps "$SLIPPAGE_BPS" \
    --yes \
    "$INPUT_MINT" "$OUTPUT_MINT" "$AMOUNT"

echo "Swap completed!"
