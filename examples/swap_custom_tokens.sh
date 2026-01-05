#!/bin/bash
# Example: Swap between any two tokens using mint addresses
#
# Prerequisites:
#   1. Set TITAN_TOKEN environment variable
#   2. Have a Solana keypair JSON file
#
# Usage:
#   ./examples/swap_custom_tokens.sh <keypair> <input_mint> <output_mint> <amount>
#
# Example (swap BONK to SOL):
#   ./examples/swap_custom_tokens.sh ~/.config/solana/id.json \
#       DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263 \
#       So11111111111111111111111111111111111111112 \
#       1000000000000

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

KEYPAIR_PATH="$1"
INPUT_MINT="$2"
OUTPUT_MINT="$3"
AMOUNT="$4"

if [ -z "$KEYPAIR_PATH" ] || [ -z "$INPUT_MINT" ] || [ -z "$OUTPUT_MINT" ] || [ -z "$AMOUNT" ]; then
    echo "Usage: $0 <keypair_path> <input_mint> <output_mint> <amount>"
    echo ""
    echo "Common token mints:"
    echo "  SOL/WSOL: So11111111111111111111111111111111111111112"
    echo "  USDC:     EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    echo "  USDT:     Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
    echo "  BONK:     DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
    echo "  JUP:      JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN"
    exit 1
fi

echo "Swapping tokens..."
echo "  Input mint:  $INPUT_MINT"
echo "  Output mint: $OUTPUT_MINT"
echo "  Amount:      $AMOUNT"

cargo run --features cli --bin titan-cli -- swap \
    --keypair "$KEYPAIR_PATH" \
    --slippage-bps 50 \
    "$INPUT_MINT" "$OUTPUT_MINT" "$AMOUNT"
