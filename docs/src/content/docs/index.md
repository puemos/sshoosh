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
ssh -p 2222 "$USER+TOKEN@127.0.0.1"
```

Connect as `username+TOKEN@host` with the one-time bootstrap token to create the first owner, create `#general`, and auto-join the owner to it. Additional unknown SSH keys must also connect as `username+invite-token@host`, or an owner/admin can add a key directly to an existing account. Unknown keys without a token are rejected before any account rows are written. `#general` is mandatory for activated users and cannot be left, archived, or made private.

## Quick Deploy

`sshoosh` is a raw SSH/TCP server, not an HTTP app. Deploy it on a host that can run a long-lived process and expose TCP to the port where `sshoosh serve` listens. Keep `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY` on persistent storage; the optional `SSHOOSH_ENCRYPTION_KEY` must also be stable if encryption is enabled. Losing the server key makes SSH clients warn that the host key changed.

| Target | Good fit | Setup notes |
| --- | --- | --- |
| Local or LAN | Testing, homelab, private network use | Bind `0.0.0.0:2222`, open the firewall if needed, and connect by host name or LAN IP. |
| Local plus expose | Temporary sharing from a laptop or workstation | Use a raw TCP tunnel such as [ngrok TCP](https://ngrok.com/docs/universal-gateway/tcp), [Cloudflare Tunnel arbitrary TCP](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/non-http/cloudflared-authentication/arbitrary-tcp/), Tailscale, or an SSH reverse tunnel. |
| VPS with systemd | Recommended production path | Install the release binary, store state under `/var/lib/sshoosh`, then use `packaging/sshoosh.service`. |
| PaaS or container host | Works only with raw TCP and persistent storage | Railway TCP Proxy and Fly public TCP services can fit. HTTP-only app hosts need a raw TCP feature. |
| Static or serverless hosts | Usually not a fit | `sshoosh` needs inbound SSH/TCP and process state, not request/response HTTP execution. |

Local or LAN:

```sh
SSHOOSH_DB=./sshoosh.sqlite \
SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519 \
sshoosh serve --host 0.0.0.0 --port 2222

ssh -p 2222 "$USER@<host-or-lan-ip>"
```

Expose a local server with ngrok TCP:

```sh
ngrok tcp 2222
ssh -p <ngrok-port> "$USER@<ngrok-host>"
```

Expose through Cloudflare Tunnel TCP. The server keeps an outbound tunnel open; clients run `cloudflared access tcp` locally and then SSH to the local forwarded port:

```sh
cloudflared tunnel --hostname sshoosh.example.com --url tcp://localhost:2222
cloudflared access tcp --hostname sshoosh.example.com --url localhost:9222
ssh -p 9222 "$USER@127.0.0.1"
```

For Tailscale, prefer private tailnet access to the machine running `sshoosh`. Tailscale Funnel can be used only when its allowed public TCP ports and TLS behavior fit your client path:

```sh
ssh -p 2222 "$USER@<tailscale-machine-name-or-ip>"
tailscale funnel --tcp=<allowed-funnel-port> tcp://localhost:2222
```

An SSH reverse tunnel is useful when you control a relay host that allows remote forwarded ports:

```sh
ssh -N -R <public-port>:localhost:2222 user@bastion.example.com
ssh -p <public-port> "$USER@bastion.example.com"
```

For a VPS, bootstrap against the same production paths the service will use, then enable the systemd service below:

```sh
cargo build --release
sudo install -m 0755 target/release/sshoosh /usr/local/bin/sshoosh
sudo useradd --system --home /var/lib/sshoosh --shell /usr/sbin/nologin sshoosh 2>/dev/null || true
sudo install -d -o sshoosh -g sshoosh /var/lib/sshoosh
sudo -u sshoosh env \
  SSHOOSH_DB=/var/lib/sshoosh/sshoosh.sqlite \
  SSHOOSH_SERVER_KEY=/var/lib/sshoosh/sshoosh_server_ed25519 \
  /usr/local/bin/sshoosh bootstrap-token
```

On Railway, set a fixed internal port such as `SSHOOSH_PORT=2222`, mount persistent storage for `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`, then enable TCP Proxy to that port and connect to the generated proxy host and port. On Fly, use a persistent volume such as `/data`, set `SSHOOSH_DB=/data/sshoosh.sqlite` and `SSHOOSH_SERVER_KEY=/data/sshoosh_server_ed25519`, and configure a raw TCP pass-through service with no HTTP/TLS handlers or PROXY protocol in front of `sshoosh`.

## Connection Resilience

`sshoosh` runs over SSH/TCP, so a client cannot resume the exact same TCP session after the network path is broken. Reconnecting is safe: durable chat state lives in the database, and the client will return to the current account after authentication. Unsaved text in the local compose box may be lost if the terminal disconnects.

For laptops, mobile hotspots, tunnels, or NATs that briefly stop passing traffic, configure OpenSSH protocol keepalives on the client. This keeps idle sessions from being closed too aggressively and gives SSH a longer window before it decides the server is unreachable:

```text
Host sshoosh
  HostName sshoosh.example.com
  Port 2222
  User alice
  ServerAliveInterval 30
  ServerAliveCountMax 10
  TCPKeepAlive no
```

`ServerAliveInterval` sends encrypted SSH keepalive messages after an idle period, and `ServerAliveCountMax` controls how many unanswered messages are tolerated before SSH exits. The example above waits roughly five minutes before giving up. `TCPKeepAlive no` avoids relying on lower-level TCP keepalives, which can make temporary route loss look like a hard failure.

OpenSSH keepalives do not auto-reconnect after `ssh` exits. If you want a wrapper to retry, use a small loop or `autossh` with OpenSSH keepalives:

```sh
autossh -M 0 \
  -o ServerAliveInterval=30 \
  -o ServerAliveCountMax=3 \
  -o TCPKeepAlive=no \
  -p 2222 alice@sshoosh.example.com
```

Mosh is not a direct fit for `sshoosh`: it bootstraps over SSH and then starts `mosh-server` in a normal remote shell, while `sshoosh` is the SSH application itself.

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
SSHOOSH_MAX_CONNECTIONS=256
SSHOOSH_MAX_CONNECTIONS_PER_IP=32
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
