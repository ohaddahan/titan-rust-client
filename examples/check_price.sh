#!/bin/bash
# Example: Check swap price without executing
#
# Use this to check current prices before swapping.
#
# Usage:
#   ./examples/check_price.sh [input_mint] [output_mint] [amount]
#
# Examples:
#   ./examples/check_price.sh SOL USDC 1000000000
#   ./examples/check_price.sh USDC SOL 1000000

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

INPUT_MINT="${1:-SOL}"
OUTPUT_MINT="${2:-USDC}"
AMOUNT="${3:-1000000000}"

echo "Checking price for $INPUT_MINT -> $OUTPUT_MINT..."

cargo run --features cli --bin titan-cli -- price \
    "$INPUT_MINT" "$OUTPUT_MINT" "$AMOUNT"
