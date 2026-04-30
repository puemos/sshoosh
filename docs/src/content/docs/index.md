---
title: Overview
description: Install, run, and operate sshoosh, the SSH-native TUI workspace chat.
---

`sshoosh` is a self-hosted SSH/TUI workspace chat. Users connect with an SSH key and get a terminal UI for explicit-membership channels, thread-first discussions, direct messages, notifications, mentions, reactions, unread state, full-text search, presence, export, and administration.

## Quick Start

Run the server and connect with any SSH client:

```sh
sshoosh bootstrap-token
cargo run -- serve --host 0.0.0.0 --port 2222
ssh -p 2222 "$USER@127.0.0.1"
```

Enter the one-time bootstrap token in the setup screen to create the first owner, create `#general`, and auto-join the owner to it. Additional unknown SSH keys connect as `username@host` and stay blocked in setup until they enter an invite token, or the key can be attached to an existing account by an owner/admin. The legacy `username+token@host` form is still supported. `#general` is mandatory for activated users and cannot be left, archived, or made private.

## Configuration

Every server flag can also be set with an environment variable:

```sh
SSHOOSH_DB=/var/lib/sshoosh/sshoosh.sqlite
SSHOOSH_DATABASE_URL=libsql://example.turso.io
SSHOOSH_DATABASE_AUTH_TOKEN=...
SSHOOSH_NODE_ID=sshoosh-1
SSHOOSH_ENCRYPTION_KEY=...
SSHOOSH_MASTER_LEASE_TTL_SECS=15
SSHOOSH_MASTER_HEARTBEAT_SECS=5
SSHOOSH_HOST=0.0.0.0
SSHOOSH_PORT=2222
SSHOOSH_SERVER_KEY=/var/lib/sshoosh/sshoosh_server_ed25519
SSHOOSH_NO_MOUSE=false
```

Use `--no-mouse` or `SSHOOSH_NO_MOUSE=true` if your terminal has problematic mouse reporting. UTF-8 support is required. Mouse support, bracketed paste, OSC52 copy, OSC8 hyperlinks, and cursor shape hints improve the experience but are optional.

## Commands

Core CLI commands:

```sh
sshoosh serve
sshoosh bootstrap-token
sshoosh doctor
sshoosh doctor --repair-search
sshoosh backup /path/to/backup.sqlite
sshoosh master status
sshoosh encrypt migrate
sshoosh invite --role member --ttl-hours 24
```

Protected CLI commands require `--actor ownername` to attribute the action to a specific active account.

| Area | CLI examples |
| --- | --- |
| Users | `sshoosh users list`, `sshoosh users role alice admin`, `sshoosh users disable alice` |
| Keys | `sshoosh keys list`, `sshoosh keys add "ssh-ed25519 AAAA..." --username alice`, `sshoosh keys revoke <key>` |
| Invites | `sshoosh invites create --role admin --ttl-hours 2`, `sshoosh invites revoke <invite-id>` |
| Channels | `sshoosh channels create engineering`, `sshoosh channels create ops-secret --private`, `sshoosh channels add-member ops-secret alice` |
| Notifications | `sshoosh notifications list --actor alice`, `sshoosh notifications mark-read --actor alice` |
| Export | `sshoosh export --format json --out export.json --include-audit`, `sshoosh export --format markdown --out export.md` |

Common TUI commands:

```text
/invite new [member|admin] [ttl-hours]
/channel new name
/channel private name
/channel list
/channel join #channel
/channel leave [#channel]
/thread new title
/dm open @user
/search query
/notification mentions
/notification list
/audit
```

In compose mode, `Ctrl-X E` prefills an edit command for your latest comment in the current thread or your latest message in the current DM. With mouse support enabled, right-click one of your comments or DM messages to open the message menu, then choose edit or delete; deletes require confirmation.

## Membership And Channels

Beyond mandatory `#general`, public channels use explicit membership: users can discover public channels with `channels list`, but content is visible and searchable only after joining.

Private channels require owner/admin management through the CLI or TUI commands:

```text
/channel members #channel
/channel add #channel @user
/channel remove #channel @user
```

## Notifications

`sshoosh` creates durable in-app notifications for `@username` mentions, new direct messages, and replies to threads you participate in. Muted threads and muted DMs suppress new notifications until the mute expires.

Notification and mention lists include a source column for the originating channel, thread, or DM. With mouse support enabled, click a source row to open it; public channels are joined automatically when needed, while private channels still require membership. The topbar notification and mention counters are also clickable shortcuts to their lists.

Terminal system notifications are opt-in per account. Use `/notification terminal on`, `/notification terminal off`, or `/notification terminal status` in the TUI. sshoosh sends terminal notification escape sequences to the SSH client and falls back to the terminal bell where desktop notifications are unsupported.

## Backup, Export, And systemd

Use SQLite backups for operational recovery and exports for portable archives:

```sh
sshoosh backup /var/backups/sshoosh.sqlite
sshoosh export --format json --out /var/backups/sshoosh.json --include-audit
sshoosh export --format markdown --out /var/backups/sshoosh.md
```

The release artifact is a single `sshoosh` binary. Runtime state is just the SQLite database and SSH host key configured by `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`.

For remote libSQL/Turso, set `SSHOOSH_DATABASE_URL` and `SSHOOSH_DATABASE_AUTH_TOKEN`; this overrides `SSHOOSH_DB`. Multiple servers may share the same database, and all nodes accept SSH sessions and writes through the shared SQLite/libSQL transaction layer. They still contend for the `main` master lease for singleton maintenance commands such as encryption migration. Use stable `SSHOOSH_NODE_ID` values in production.

Set `SSHOOSH_ENCRYPTION_KEY` to a base64url 32-byte key to encrypt source content fields. Run `sshoosh encrypt migrate` once for existing plaintext rows. Search index columns intentionally remain plaintext so FTS works, which means search data remains sensitive at rest.

For systemd deployments, copy `packaging/sshoosh.service` to `/etc/systemd/system/sshoosh.service`, adjust the binary path if needed, then:

```sh
sudo install -d -o sshoosh -g sshoosh /var/lib/sshoosh
sudo systemctl daemon-reload
sudo systemctl enable --now sshoosh
```

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

For reloadable local development:

```sh
cargo run -- dev --host 127.0.0.1 --port 2222
cargo run -- dev-ssh --host 127.0.0.1 --port 2222
```
