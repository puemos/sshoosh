# AGENTS.md

## Scope

These instructions apply to the whole repository. Keep them concise and durable; update this file when build, test, release, architecture, or security expectations change.

## Project Intent

`sshoosh` is a self-hosted SSH/TUI workspace chat for small teams and operators. Users connect with SSH keys and work inside a dense terminal UI for channels, threads, DMs, notifications, mentions, reactions, unread state, search, exports, and administration.

The runtime should stay small and operator-friendly: one Rust binary, one SQLite/libSQL database, one SSH host key, and optional app-level encryption.

## Repo Map

- `src/cli/`: clap CLI definitions, command dispatch, local dev helpers, and sensitive file writing.
- `src/ssh/`: SSH server/session transport, authentication handoff, render loop integration, and SSH-facing action formatting.
- `src/app/`: TUI application state, input handling, slash commands, navigation, rendering, theme, and terminal interaction behavior.
- `src/client/`: `ClientSession` facade used by the TUI/SSH layer to call service behavior.
- `src/service/`: durable product behavior, permissions, loaders, write operations, notifications/reactions, audit/export, runtime, and `ServerState`.
- `src/domain/`: shared domain models.
- `src/db.rs`: database connection, migrations, query wrapper, encryption, backups, doctor checks, master lease, and local file permissions.
- `migrations/`: SQL migrations embedded by `src/db.rs`.
- `tests/integration.rs`: end-to-end service, persistence, SSH, auth, security, and CLI behavior coverage.
- `docs/`: Astro docs site. Use `pnpm`; do not edit `docs/node_modules`, `docs/dist`, or `docs/.astro`.
- `packaging/`, `Dockerfile`, `install.sh`, `.github/workflows/`: release, install, systemd, Docker, Pages, and CI automation.

## Working Rules

- Prefer existing module patterns over new abstractions. Keep edits narrowly scoped to the behavior being changed.
- Use `rg`/`rg --files` for search. Exclude generated or ignored paths such as `target/`, `docs/node_modules/`, `docs/dist/`, and `docs/.astro/`.
- Do not commit secrets, local databases, SSH host keys, release artifacts, or generated dependency directories.
- Protect `SSHOOSH_DB`, `SSHOOSH_DATABASE_AUTH_TOKEN`, `SSHOOSH_ENCRYPTION_KEY`, `SSHOOSH_SERVER_KEY`, exports, backups, and provider snapshots as sensitive data.
- Do not add a license file without an explicit owner decision.
- Keep user-facing commands, docs, CLI help, and TUI slash command references synchronized when behavior changes.

## Architecture Boundaries

- Durable product behavior belongs in `ServerState` and the service modules. Avoid putting persistence, permissions, audit, notification, or membership rules in render, input, SSH, or CLI code.
- `src/app` owns TUI state, rendering, input, slash command parsing/autocomplete, navigation, hit maps, and terminal affordances.
- `src/ssh` should remain transport/session glue: SSH auth, channels, render loop, terminal IO, and mapping TUI actions to client calls.
- `src/cli` should parse/administer commands and delegate behavior to service/database APIs rather than duplicating business logic.
- `ClientSession` is the app-facing facade over service behavior. TUI actions should route through it instead of reaching directly into `ServerState`.
- Database writes should go through the query wrapper in `src/db.rs` so encryption, master/standby fencing, and local file protections remain effective.
- Schema changes must update SQL migrations, the embedded migration list in `src/db.rs`, and tests that prove upgrades and persistence still work.

## Product And Security Invariants

- Unknown SSH keys must redeem a bootstrap or invite token via the SSH keyboard-interactive prompt before any account or key row is written. Tokens must never be parsed out of the SSH user field, sent over `auth_password`, or stored in plaintext in logs.
- The first activated account is bootstrapped with a one-time bootstrap token and becomes owner.
- `#general` is mandatory for activated users and must not become private, leaveable, archived, or deleted.
- Public channels are discoverable, but content visibility and searchability depend on explicit membership.
- Private channel content, mentions, notifications, search results, and source links must stay visible only to members.
- Owner/admin boundaries matter. Do not allow admins to mint owners/admins or remove the last active owner.
- Admin and security-sensitive actions should be audited.
- Unread counts, notification read state, muted threads/DMs, saved messages, and source navigation should stay consistent after edits/deletes.
- `SSHOOSH_ENCRYPTION_KEY` encrypts source content fields, but `search_index` remains plaintext by design so full-text search works. Treat search data as sensitive at rest.
- Local SQLite files, backups, generated bootstrap tokens, and server keys should use owner-only permissions where applicable.

## TUI Design Principles

- Preserve the dense, terminal-native operator UI. Avoid marketing-style copy, sparse layouts, or decorative UI.
- Use `src/app/theme.rs` and existing render helpers for colors, panels, message cards, badges, selection, and status styles.
- Keep rows compact and scannable. Text must truncate or wrap predictably at narrow terminal widths without shifting layout unexpectedly.
- Maintain keyboard and mouse behavior together. When changing an interaction, update hit maps, keyboard handling, mouse handling, and tests as needed.
- Keep visual states explicit: selected, focused, unread, muted, pinned, archived, private, saved, online/away/offline, and destructive confirmation states.
- Sanitize terminal-visible text and escape-sequence payloads; never let stored content inject terminal controls.
- Prefer deterministic rendering tests for layout and interaction changes.

## Slash Commands And UI Actions

- New or changed slash commands usually need coordinated updates in command specs, parsing, autocomplete/help, action handling, and tests.
- Keep command aliases documented in code and docs when they are user-facing.
- Commands that mutate durable state should surface clear errors and route through service permissions.
- For edit/delete flows, keep ownership and confirmation behavior explicit in both keyboard and mouse paths.

## Database And Migrations

- The project uses SQLite/libSQL. `SSHOOSH_DATABASE_URL` overrides `SSHOOSH_DB`; remote URLs require `SSHOOSH_DATABASE_AUTH_TOKEN`.
- Multi-node remote deployments rely on stable `SSHOOSH_NODE_ID` values and master lease fencing. Do not bypass standby write protections.
- For schema work, keep the baseline schema in `migrations/20260430000000_initial.sql` coherent and add forward migrations when existing databases need an upgrade path.
- When adding a migration file, add the `include_str!` wiring and application order in `src/db.rs`, plus tests for fresh and upgraded database behavior when relevant.
- Keep FTS/search-index maintenance in step with create/edit/delete operations for threads, comments, and DMs.

## Validation

Select the smallest relevant gate that proves the change. For PR-ready Rust changes, prefer `just ci`.

- Format check: `just format`
- Apply formatting: `just fmt`
- Lint: `just lint`
- Tests: `just test`
- Release build: `just build`
- Full Rust CI equivalent: `just ci`
- Docs build: `cd docs && pnpm install --frozen-lockfile && pnpm build`

Focused checks are useful while iterating:

- CLI parsing: `cargo test cli::tests`
- Slash commands: `cargo test app::commands::tests`
- App/TUI behavior: `cargo test app::tests`
- Render helpers: `cargo test app::render::tests`
- Terminal escapes/sanitization: `cargo test terminal::tests`
- Service unit behavior: `cargo test service::tests`
- Integration behavior: `cargo test --test integration`
- End-to-end SSH behavior: `cargo test --test integration ssh_e2e_authenticates_renders_and_creates_thread`

Run docs checks when touching `docs/`, docs workflows, or public command documentation. Run release build or `just ci` before release-facing changes.

## Local Development

- Reloadable local server: `cargo run -- dev --host 127.0.0.1 --port 2222`
- Auto-reconnecting local SSH client: `cargo run -- dev-ssh --host 127.0.0.1 --port 2222`
- DB performance seed/bench helper: `cargo run -- dev-db-bench --users 50 --channels 50 --threads 1000 --comments 100000 --dms 5000`

Useful local environment variables:

```sh
SSHOOSH_DB=./sshoosh.sqlite
SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519
SSHOOSH_HOST=0.0.0.0
SSHOOSH_PORT=2222
SSHOOSH_NO_MOUSE=false
```

## Release Guide

Releases are tag-driven. The release tag format is `vX.Y.Z`.

Before tagging:

- Update versioned metadata if needed, especially `Cargo.toml`, docs, packaging text, and any release notes the owner asks for.
- Confirm `Cargo.lock` is current.
- Run `just ci`.
- If docs changed, run `cd docs && pnpm install --frozen-lockfile && pnpm build`.
- Confirm the intended commit is on `main` and has the changes to release.

Tag and push:

```sh
git tag vX.Y.Z
git push origin vX.Y.Z
```

What automation does after the tag push:

- `.github/workflows/release.yml` builds release binaries for Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.
- The release workflow packages Unix targets as `.tar.gz`, Windows as `.zip`, publishes GitHub Release assets, and uploads `SHA256SUMS.txt`.
- The release workflow then checks out `puemos/homebrew-tap`, renders `Formula/sshoosh.rb` from `packaging/homebrew/sshoosh.rb.template`, updates the tap README, commits, and pushes. This depends on the `GH_TOKEN` secret.
- `.github/workflows/docker.yml` also runs on `v*.*.*` tags and publishes `ghcr.io/puemos/sshoosh` images for linux/amd64 and linux/arm64. Version tags also publish `latest`.

After release:

- Verify the GitHub Release contains all expected artifacts and `SHA256SUMS.txt`.
- Verify installer download and checksum flow with `./install.sh --version vX.Y.Z --dir <tmp-dir>`.
- Verify the GHCR Docker image tags exist.
- Verify `puemos/homebrew-tap` updated and `brew install puemos/tap/sshoosh` resolves after Homebrew metadata catches up.
- If release automation fails after publishing partial assets, inspect the failed workflow before retagging. Prefer fixing forward with the same tag only when it is safe and no public consumers have fetched broken artifacts.
