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
ssh -p 2222 "$USER+<bootstrap-token>@127.0.0.1"
```

The bootstrap token is one-time and creates the first owner, creates `#general`, and auto-joins the owner to it. Additional unknown SSH keys must connect as `username+invite-token` or be attached to an existing account by an owner/admin.

## Configuration

Every server flag can also be set with an environment variable:

```sh
SSHOOSH_DB=/var/lib/sshoosh/sshoosh.sqlite
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

## Membership And Channels

`#general` is mandatory for activated users and cannot be left, archived, or made private. Public channels use explicit membership: users can discover public channels with `channels list`, but content is visible and searchable only after joining.

Private channels require owner/admin management through the CLI or TUI commands:

```text
/channel members #channel
/channel add #channel @user
/channel remove #channel @user
```

## Notifications

`sshoosh` creates durable in-app notifications for `@username` mentions, new direct messages, and replies to threads you participate in. Muted threads and muted DMs suppress new notifications until the mute expires.

## Backup, Export, And systemd

Use SQLite backups for operational recovery and exports for portable archives:

```sh
sshoosh backup /var/backups/sshoosh.sqlite
sshoosh export --format json --out /var/backups/sshoosh.json --include-audit
sshoosh export --format markdown --out /var/backups/sshoosh.md
```

The release artifact is a single `sshoosh` binary. Runtime state is just the SQLite database and SSH host key configured by `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`.

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
