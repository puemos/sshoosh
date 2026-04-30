# Contributing

## Development Checks

Run these before sending changes:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

## Local Workflow

Use `cargo run -- dev --host 127.0.0.1 --port 2222` for the reloadable server and `cargo run -- dev-ssh --host 127.0.0.1 --port 2222` for an auto-reconnecting client.

## Database Changes

V1 uses a clean initial SQLite schema. Until a stable release requires migrations from production databases, keep schema changes explicit in `migrations/20260430000000_initial.sql` and update integration tests for behavior changes.

## Code Style

- Prefer existing service and TUI command patterns.
- Keep admin actions audited.
- Add focused tests for visibility, unread state, notifications, and persistence when changing product behavior.
- Do not add a license file without an explicit owner decision.
