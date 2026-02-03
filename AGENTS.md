# Repository Guidelines

## Project Structure & Module Organization
- `src/` holds the library code; `src/lib.rs` is the entry point and modules live in files like `client.rs`, `connection.rs`, and `stream.rs`.
- `src/bin/titan_cli.rs` contains the CLI used for live API checks.
- `tests/` contains integration-style tests; `tests/common/` has shared helpers.
- `examples/` and `scripts/` include shell helpers for API calls and test workflows.
- `Cargo.toml`/`Cargo.lock` define dependencies and features (`cli`, `solana`).

## Build, Test, and Development Commands
- `cargo build` / `cargo build --release`: compile the library/CLI.
- `cargo run --features cli --bin titan-cli -- --help`: run the CLI (requires the `cli` feature).
- `cargo test`: run the full test suite.
- `cargo test --test api`: run a single integration test file.
- `./scripts/tests.sh`: run tests with `--include-ignored --no-capture`.
- `./scripts/integration_check.sh [info|venues|providers|price|concurrent|all]`: run live API smoke checks (requires `TITAN_TOKEN`; optional `TITAN_URL`, read from `.env`).
- `cargo fmt` / `cargo clippy --all-targets`: format and lint.

## Coding Style & Naming Conventions
- Rust 2021 edition; use `cargo fmt` for formatting (4-space indentation, rustfmt defaults).
- Naming: `snake_case` for functions/vars, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep files small (~200 lines); prefer splitting modules and keep `mod.rs` files declarative.
- Error handling uses `Result` with `TitanClientError`; avoid `panic!`.
- Use `#[tracing::instrument(skip_all)]` on public APIs and avoid `println!`/`dbg!`.

## Testing Guidelines
- Tests use the standard Rust test harness; prefer integration tests in `tests/` that exercise the public API.
- Live API tests require valid credentials (`TITAN_TOKEN`); avoid asserting on non-deterministic fields.
- Name tests by behavior (e.g., `real_get_swap_price`) and keep helpers in `tests/common/`.

## Commit & Pull Request Guidelines
- Commit history is brief and informal; keep subjects short and descriptive (one line).
- For PRs, include: purpose, key changes, and how you tested (e.g., `cargo test`, `integration_check.sh`).
- Do not commit secrets; `.env` is for local use only.

## Configuration & Security Notes
- CLI auth uses `TITAN_TOKEN`; `TITAN_URL` defaults to `wss://api.titan.ag/api/v1/ws`.
- Treat tokens as sensitive; use environment variables or `.env`, never source control.
