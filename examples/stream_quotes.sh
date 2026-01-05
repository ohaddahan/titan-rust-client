#!/bin/bash
# Example: Stream live quotes
#
# Continuously streams swap quotes to monitor price changes.
# Press Ctrl+C to stop.
#
# Usage:
#   ./examples/stream_quotes.sh [input_mint] [output_mint] [amount]
#
# Examples:
#   ./examples/stream_quotes.sh SOL USDC 1000000000
#   ./examples/stream_quotes.sh USDC SOL 1000000

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

echo "Streaming quotes for $INPUT_MINT -> $OUTPUT_MINT (Ctrl+C to stop)..."

cargo run --features cli --bin titan-cli -- stream \
    "$INPUT_MINT" "$OUTPUT_MINT" "$AMOUNT"
