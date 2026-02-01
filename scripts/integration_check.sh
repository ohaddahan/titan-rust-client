#!/bin/bash
# Mirrors the integration tests so you can verify API behavior before running tests.
# Usage: ./scripts/integration_check.sh [test_name]
# Available: info, venues, providers, price, concurrent, all
# Requires: TITAN_TOKEN (and optionally TITAN_URL) in .env or environment.

set -e

if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

if [ -z "$TITAN_TOKEN" ]; then
    echo "Error: TITAN_TOKEN not set"
    exit 1
fi

TITAN_URL="${TITAN_URL:-wss://api.titan.ag/api/v1/ws}"
CLI="cargo run --features cli --bin titan-cli --"

run_info() {
    echo "=== get_info ==="
    $CLI info
    echo ""
}

run_venues() {
    echo "=== get_venues ==="
    $CLI venues
    echo ""
}

run_providers() {
    echo "=== list_providers ==="
    $CLI providers
    echo ""
}

run_price() {
    echo "=== get_swap_price (SOL->USDC, 1_000_000 lamports) ==="
    echo "Mirrors: real_get_swap_price — amount=1_000_000, asserts amount_in==1_000_000 and amount_out>0"
    $CLI price SOL USDC 1000000
    echo ""
}

run_concurrent() {
    echo "=== concurrent one-shot requests ==="
    $CLI info &
    PID1=$!
    $CLI venues &
    PID2=$!
    $CLI providers &
    PID3=$!
    wait $PID1 $PID2 $PID3
    echo ""
}

case "${1:-all}" in
    info)       run_info ;;
    venues)     run_venues ;;
    providers)  run_providers ;;
    price)      run_price ;;
    concurrent) run_concurrent ;;
    all)
        run_info
        run_venues
        run_providers
        run_price
        run_concurrent
        echo "=== All checks passed ==="
        ;;
    *)
        echo "Unknown test: $1"
        echo "Available: info, venues, providers, price, concurrent, all"
        exit 1
        ;;
esac
