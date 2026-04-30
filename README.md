# sshoosh

`sshoosh` is a self-hosted SSH/TUI workspace chat. Users connect with an SSH key and get a terminal UI for explicit-membership channels, thread-first discussions, direct messages, notifications, mentions, reactions, unread state, FTS search, presence, export, webhooks, and administration.

## Quick Start

```sh
cargo run -- serve --host 0.0.0.0 --port 2222
ssh -p 2222 "$USER@127.0.0.1"
```

The first SSH key to connect becomes the owner, creates `#general`, and is auto-joined to it. Additional SSH keys create pending accounts until the user accepts an invite or an admin attaches the key to an existing account. `#general` is mandatory for activated users and cannot be left, archived, or made private.

## Configuration

All flags can also be set with environment variables:

```sh
SSHOOSH_DB=/var/lib/sshoosh/sshoosh.sqlite
SSHOOSH_HOST=0.0.0.0
SSHOOSH_PORT=2222
SSHOOSH_SERVER_KEY=/var/lib/sshoosh/sshoosh_server_ed25519
SSHOOSH_NO_MOUSE=false
```

## CLI

Core commands:

```sh
sshoosh serve
sshoosh doctor
sshoosh backup /path/to/backup.sqlite
sshoosh invite --role member --ttl-hours 24
```

Admin commands default to the first active owner/admin. Use `--actor ownername` to attribute an action to a specific owner/admin.

```sh
sshoosh users list
sshoosh users rename alice alice-prod
sshoosh users display-name alice "Alice Lee"
sshoosh users disable alice
sshoosh users role alice admin

sshoosh keys list
sshoosh keys add "ssh-ed25519 AAAA..." --username alice --label laptop
sshoosh keys label <key-id-or-fingerprint> desktop
sshoosh keys attach <pending-key-id-or-fingerprint> alice
sshoosh keys revoke <key-id-or-fingerprint>

sshoosh invites list
sshoosh invites create --role admin --ttl-hours 2
sshoosh invites revoke <invite-id>

sshoosh channels list
sshoosh channels create engineering
sshoosh channels create ops-secret --private
sshoosh channels join engineering --actor alice
sshoosh channels leave engineering --actor alice
sshoosh channels rename engineering eng
sshoosh channels topic eng "Build and release coordination"
sshoosh channels archive eng
sshoosh channels unarchive eng
sshoosh channels members ops-secret
sshoosh channels add-member ops-secret alice
sshoosh channels remove-member ops-secret alice

sshoosh notifications list --actor alice
sshoosh notifications mark-read --actor alice
sshoosh webhooks list
sshoosh webhooks add ops https://example.com/sshoosh
sshoosh webhooks test <webhook-id>
sshoosh webhooks remove <webhook-id>
sshoosh audit list --limit 100
sshoosh export --format json --out /path/to/export.json --include-audit
sshoosh export --format markdown --out /path/to/export.md
```

Public channels use explicit membership. A user can discover public channels with `channels list`, but content is visible and searchable only after joining.

## TUI Commands

Common commands:

```text
/invite new [member|admin] [ttl-hours]
/invite list
/invite revoke invite-id
/channel new name
/channel private name
/channel list
/channel join #channel
/channel leave [#channel]
/channel topic [#channel] topic
/channel rename [#channel] name
/channel archive [#channel]
/channel unarchive #channel
/thread new title
/thread rename title
/dm open @user
/search query
```

Admin and lifecycle commands:

```text
/user list
/user profile display-name
/user username username
/user disable @user
/user enable @user
/user role @user owner|admin|member
/key list
/key my
/key add ssh-ed25519... [| label]
/key label key-id label
/key revoke key-id-or-fingerprint
/channel members #channel
/channel add #channel @user
/channel remove #channel @user
/comment edit index body
/comment delete index
/dm edit index body
/dm delete index
/thread delete
/thread archive
/thread unarchive
/thread pin
/thread unpin
/thread mute [hours]
/thread unmute
/thread save
/thread unsave
/reaction add emoji [comment-or-message-index]
/reaction remove emoji [comment-or-message-index]
/notification mentions
/notification list
/notification read [notification-id]
/webhook list
/webhook add name url
/webhook remove webhook-id
/audit list
/more
/older
```

Threads and DMs are marked read when opened in the detail view. Manual unread remains until the item is opened again or explicitly marked read. Deleted comments/messages are excluded from unread counts.

Bare URLs and Markdown links render as OSC8 terminal hyperlinks where supported. `sshoosh` does not open links on the server; use terminal link support or copy the visible URL.

## Notifications and Webhooks

V1 creates durable in-app notifications for `@username` mentions, new DMs, and replies to threads you participate in. Muted threads and muted DMs suppress new notifications until the mute expires. Use `/notification list`, `/notification mentions`, and `/notification read` in the TUI or `sshoosh notifications ...` from the CLI.

Outgoing webhooks are admin-managed. Each notification queues delivery jobs for enabled webhooks, and a background worker retries failed deliveries with exponential backoff before marking them failed. Delivery history is visible with `/webhook list` or `sshoosh webhooks list`.

Webhook payloads are JSON and include the notification kind, title, body, and notification id. Use HTTPS endpoints in production.

## Backup and Export

Use SQLite backups for operational recovery and exports for portable archives:

```sh
sshoosh backup /var/backups/sshoosh.sqlite
sshoosh export --format json --out /var/backups/sshoosh.json --include-audit
sshoosh export --format markdown --out /var/backups/sshoosh.md
```

The JSON/Markdown export includes users, channels, threads, comments, DMs, mentions, reactions, notifications, webhooks, and optionally audit rows. It is not an import format.

## Terminal Requirements

Use an SSH client and terminal with UTF-8 support. Mouse support, bracketed paste, OSC52 copy, OSC8 hyperlinks, and cursor shape hints improve the experience but are optional. Set `--no-mouse` or `SSHOOSH_NO_MOUSE=true` if your terminal has problematic mouse reporting.

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

## Release Binary

```sh
cargo build --release
install -m 0755 target/release/sshoosh /usr/local/bin/sshoosh
```

The release artifact is a single `sshoosh` binary. Runtime state is just the SQLite database and SSH host key configured by `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`.

## systemd

Copy `packaging/sshoosh.service` to `/etc/systemd/system/sshoosh.service`, adjust the binary path if needed, then:

```sh
sudo install -d -o sshoosh -g sshoosh /var/lib/sshoosh
sudo systemctl daemon-reload
sudo systemctl enable --now sshoosh
```
