---
title: User Manual
description: Complete user, admin, and operator guide for sshoosh.
---

`sshoosh` is a self-hosted workspace chat that opens directly inside SSH. There is no web app to sign into: connect with an SSH key and the terminal becomes the product.

## Start Here

Use this page based on what you are trying to do right now.

| I want to... | Start with |
| --- | --- |
| Join a workspace | First login, then using the app |
| Talk with teammates | Channels, threads, DMs, and messages |
| Catch up | Notifications, mentions, unread state, search, and saved messages |
| Manage a team | Invites, users, roles, keys, and private channels |
| Run the server | Install, daemon, Docker, config, backup, and repair |

## Join A Workspace

You need an SSH client, an SSH key, the server host and port, and either a one-time invite token or a key that an owner/admin already registered for you.

Connect to the sshoosh host:

```sh
ssh -p 2222 sshoosh.example.com
```

Use a terminal with UTF-8 support. Mouse support is optional; everything important is available from the keyboard. If your terminal has trouble with mouse reporting, the operator can run the server with `--no-mouse` or `SSHOOSH_NO_MOUSE=true`.

### First login

The first account on a new server is created with a bootstrap token:

```sh
sshoosh bootstrap-token
ssh -p 2222 sshoosh.example.com
```

Paste the token at the `Token:` prompt. The prompt uses SSH keyboard-interactive auth with input masking, so the token does not need to be placed in the username, shell history, or command line. After the token is accepted, choose your sshoosh username in the setup modal. The first activated account becomes the owner and is joined to `#general`.

New users follow the same flow with an invite token:

```sh
ssh -p 2222 sshoosh.example.com
# Paste the invite token at the "Token:" prompt,
# then choose your username in the TUI.
```

Future logins with the same SSH key go straight into the TUI. If an owner/admin pre-registers your public key with `sshoosh keys add`, you will not see the invite prompt.

### Using the app

The left workspace column contains notifications, saved messages, channels, threads, and direct messages. The main detail pane shows the selected channel thread, DM, saved message list, notification list, or search results. The compose area at the bottom is where you write messages and slash commands.

Core keyboard controls:

| Key | Action |
| --- | --- |
| `j` / `k` | Move through workspace rows |
| `h` / `l` | Collapse/back or open/expand |
| `Tab` | Toggle focus between workspace and detail panes |
| `Space` | Toggle a channel's thread list |
| `Enter` | Open the selected item or send the composed message |
| `Shift-Enter` | Insert a newline in compose |
| `/` | Start a slash command |
| `Ctrl-P` | Open the command palette |
| `?` or `/help` | Open the in-app keyboard and command help |
| `Esc` | Close the current overlay or mode |
| `q` or `Ctrl-C` | Disconnect |

When autocomplete is open, use `Up`/`Down` to move through suggestions and `Tab` to accept one. With mouse support enabled, you can click workspace rows, notification sources, saved-message sources, reaction chips, and topbar notification/mention counters. Right-click one of your messages or comments to open the edit/delete/save menu.

## Daily Use

Most work happens in channels, threads, DMs, and the bottom compose box.

### Channels and threads

Every active user belongs to `#general`. It is mandatory and cannot be left, archived, deleted, or made private.

Public channels are discoverable, but content is visible and searchable only after you join. Private channels are visible only to members.

Common channel commands:

```text
/channel list
/channel join #ops
/channel leave #ops
/channel new engineering
/channel private incident-room
/channel topic #engineering Build and deploy notes
```

Threads live inside channels and carry most discussion:

```text
/thread new Investigate deploy failure
/thread rename Updated title
/thread pin
/thread mute 4
/thread read
/thread unread
/thread archive
```

Use read/unread and mute commands to control your own attention state. Muting a thread suppresses new thread notifications until the mute expires.

### Direct messages

Open a direct message with the palette or a command:

```text
/dm open @alice
/dm mute 8
/dm read
/dm unread
```

DMs appear in the workspace list when there is conversation history or unread activity. Direct messages can create notifications for the recipient unless the DM is muted.

### Notifications and catching up

sshoosh creates durable in-app notifications for direct messages, `@username` mentions, and replies to threads you participate in. The topbar shows unread notification and mention counts.

Useful notification commands:

```text
/notification list
/notification mentions
/notification read
/notification read notification-id
/notification terminal on
/notification terminal off
/notification terminal status
```

Terminal notifications are opt-in per account. When enabled, sshoosh sends terminal notification escape sequences to the SSH client and falls back to the terminal bell where desktop notifications are unsupported.

Notification and mention rows include their source. Click a source row with the mouse, or select it from the detail list, to jump back to the originating channel thread or DM. Public channels are joined automatically when needed; private sources still require membership.

## Messages And Actions

Select a thread or DM, type in the composer, and press `Enter` to send. Use `Shift-Enter` for a multi-line message. Mention teammates with `@username`; mention autocomplete appears while composing.

Messages support lightweight inline Markdown:

```text
**bold** *italic* `code` ~~strikethrough~~ https://example.com
```

To edit your latest comment in the current thread or latest message in the current DM, press `Ctrl-X E`. You can also use explicit commands:

```text
/comment edit 2 updated text
/comment delete 2
/dm edit 4 updated text
/dm delete 4
```

Deletes require confirmation. Edit and delete permissions stay tied to ownership and role rules enforced by the server.

### Search

Search only returns content you are allowed to see:

```text
/search deploy failure
```

Private channel content, mentions, notifications, and source links stay limited to private-channel members.

Labels in messages become visible feeds. Open a feed from the workspace label list, by clicking a rendered label, or with a command:

```text
/label $deploy-2026
```

Label feeds only include thread, comment, and DM sources you can already see.

### Saved messages

Save important messages from the right-click menu or with slash commands:

```text
/save 3
/unsave 3
```

Saved messages appear in the workspace. Selecting a saved row can navigate back to the original thread comment or DM message when you still have access.

### Reactions

React from the reaction chip UI or with commands:

```text
/reaction add emoji 2
/reaction remove emoji 2
```

If you omit the index, sshoosh applies the reaction to the current thread, current comment target, or current DM context when one is available.

## Account And SSH Keys

Users can update their own display name and username:

```text
/user profile Alice Lee
/user username alice-prod
```

List and manage your SSH keys:

```text
/key my
/key add ssh-ed25519 AAAAC3... | laptop
/key label key-id-or-fingerprint work-laptop
/key revoke key-id-or-fingerprint
```

Owners/admins can manage other users and keys through TUI commands or equivalent CLI commands. Do not paste private keys into sshoosh; only public keys should be registered.

## Admin Guide

Owners and admins create invite tokens for new users:

```text
/invite new member 24
/invite new admin 2
/invite list
/invite revoke invite-id
```

The CLI equivalent is useful outside the TUI:

```sh
sshoosh invites create --role member --ttl-hours 24 --actor owner
sshoosh invites revoke invite-id --actor owner
```

Manage users and roles:

```text
/user list
/user disable @alice
/user enable @alice
/user role @alice admin
```

Admins cannot mint owners/admins beyond their permission boundary, and sshoosh prevents removing the last active owner.

Private channel membership is owner/admin managed:

```text
/channel members #incident-room
/channel add #incident-room @alice
/channel remove #incident-room @alice
```

Security-sensitive admin actions are audited. Review audit events from the TUI or CLI:

```text
/audit
```

```sh
sshoosh audit list --limit 100
```

Create exports for review or portability:

```sh
sshoosh export --format json --out export.json --include-audit
sshoosh export --format markdown --out export.md
```

## Operator Guide

Install the release binary:

```sh
curl -fsSL https://raw.githubusercontent.com/puemos/sshoosh/main/install.sh | sh
brew install puemos/tap/sshoosh
```

Run a local or LAN server:

```sh
SSHOOSH_DB=./sshoosh.sqlite \
SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519 \
sshoosh serve --host 0.0.0.0 --port 2222
```

Run the production daemon installer on a VPS:

```sh
sudo /usr/local/bin/sshoosh daemon install --binary /usr/local/bin/sshoosh
sudo systemctl status sshoosh
```

On Linux, daemon install writes `/etc/systemd/system/sshoosh.service`, `/etc/sshoosh/sshoosh.env`, and `/var/lib/sshoosh`. On macOS, it writes a root LaunchDaemon and keeps runtime state under `/var/lib/sshoosh`. Uninstall preserves data unless `--purge-data` is passed.

Run with Docker:

```sh
docker volume create sshoosh-data
docker run --rm -v sshoosh-data:/data ghcr.io/puemos/sshoosh:latest bootstrap-token
docker run -d --name sshoosh --restart unless-stopped \
  -p 2222:2222 \
  -v sshoosh-data:/data \
  ghcr.io/puemos/sshoosh:latest
```

The Docker image listens on `0.0.0.0:2222` and stores the database plus SSH host key under `/data`.

### Configuration

Common server configuration:

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
SSHOOSH_MAX_CONNECTIONS=256
SSHOOSH_MAX_CONNECTIONS_PER_IP=32
SSHOOSH_AUTH_TIMEOUT_SECS=30
SSHOOSH_MAX_AUTH_ATTEMPTS=3
SSHOOSH_MAX_UNAUTH_CONNECTIONS=32
SSHOOSH_MAX_UNAUTH_CONNECTIONS_PER_IP=4
SSHOOSH_AUTH_FAILURE_WINDOW_SECS=300
SSHOOSH_AUTH_FAILURES_BEFORE_PENALTY=5
SSHOOSH_AUTH_PENALTY_SECS=60
SSHOOSH_SERVER_KEY=/var/lib/sshoosh/sshoosh_server_ed25519
SSHOOSH_NO_MOUSE=false
```

`SSHOOSH_DATABASE_URL` overrides `SSHOOSH_DB`. Remote libSQL/Turso deployments also need `SSHOOSH_DATABASE_AUTH_TOKEN`. Multiple nodes can share the same remote database, but use stable `SSHOOSH_NODE_ID` values so master lease fencing remains predictable.

Keep `SSHOOSH_DB`, `SSHOOSH_SERVER_KEY`, `SSHOOSH_DATABASE_AUTH_TOKEN`, and `SSHOOSH_ENCRYPTION_KEY` protected. Losing the SSH host key causes SSH clients to warn that the server identity changed. Losing the encryption key makes encrypted source content unreadable.

For production, put the SSH port behind a VPN, private network, cloud firewall, or IP allowlist whenever possible. The built-in auth limits cap unauthenticated sessions, limit failed auth attempts, and temporarily penalize sources that repeatedly submit bad tokens, but they are not a replacement for upstream DDoS protection. Security logs use stable event names such as `auth_failed`, `token_redeem_failed`, `connection_rejected`, and `auth_penalty_applied`; plaintext bootstrap, invite, and device-link tokens are never logged.

Example fail2ban filter pattern:

```text
failregex = .*(auth_failed|token_redeem_failed|connection_rejected|auth_penalty_applied).*peer_ip=<HOST>.*
ignoreregex =
```

## Recovery And Resilience

### Backup, repair, and encryption

Create SQLite backups for recovery:

```sh
sshoosh backup /var/backups/sshoosh.sqlite
```

Check server health and repair search indexes when needed:

```sh
sshoosh doctor
sshoosh doctor --repair-search
```

Enable app-level encryption by setting a stable base64url 32-byte `SSHOOSH_ENCRYPTION_KEY`. For existing plaintext rows, run:

```sh
sshoosh encrypt migrate
```

Source content fields are encrypted when encryption is enabled. Search index data remains plaintext by design so full-text search works, and should still be treated as sensitive at rest.

### Connection resilience

SSH cannot resume the exact same TCP session after the network path breaks, but reconnecting is safe because durable state lives in the database.

For laptops, tunnels, and unstable networks, configure OpenSSH keepalives:

```text
Host sshoosh
  HostName sshoosh.example.com
  Port 2222
  ServerAliveInterval 30
  ServerAliveCountMax 10
  TCPKeepAlive no
```

OpenSSH keepalives do not auto-reconnect after `ssh` exits. Use `autossh` or a small shell loop if you want retry behavior.
