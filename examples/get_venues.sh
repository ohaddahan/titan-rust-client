#!/bin/bash
# Example: Get all supported DEXes/venues from Titan
#
# Lists all available trading venues with their program IDs.
# Useful for filtering swaps to specific DEXes.
#
# Usage:
#   ./examples/get_venues.sh

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

echo "Fetching supported DEXes..."

cargo run --features cli --bin titan-cli -- venues
