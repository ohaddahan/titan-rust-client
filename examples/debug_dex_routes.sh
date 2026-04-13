#!/bin/bash
# Debug which DEXes Titan routes through for a given token pair.
#
# Fetches venues, then streams quotes to show the exact DEX path.
# Useful for diagnosing missing routes when the worker-service
# filters by TITAN_ALLOWED_DEXES.
#
# Usage:
#   ./examples/debug_dex_routes.sh <input_mint> <output_mint> [amount]
#
# Examples:
#   ./examples/debug_dex_routes.sh USDC PreweJYECqtQwBtpxHL171nL2K6umo692gTm7Q3rpgF 1000000
#   ./examples/debug_dex_routes.sh SOL USDC 1000000000

set -e

# Load .env if present
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

if [ -z "$TITAN_TOKEN" ]; then
    echo "Error: TITAN_TOKEN environment variable not set"
    exit 1
fi

INPUT_MINT="${1:?Usage: $0 <input_mint> <output_mint> [amount]}"
OUTPUT_MINT="${2:?Usage: $0 <input_mint> <output_mint> [amount]}"
AMOUNT="${3:-1000000}"

echo "=== Step 1: Available Titan venues ==="
cargo run --features cli --bin titan-cli -- venues
echo ""

echo "=== Step 2: Streaming quotes for $INPUT_MINT -> $OUTPUT_MINT ($AMOUNT) ==="
echo "    (will show DEX path per route, Ctrl+C to stop)"
echo ""
cargo run --features cli --bin titan-cli -- stream \
    "$INPUT_MINT" "$OUTPUT_MINT" "$AMOUNT"
