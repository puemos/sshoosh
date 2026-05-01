<p align="center">
  <img src="docs/public/assets/sshoosh-logo-light.svg" alt="sshoosh logo" width="420">
</p>

# sshoosh

`sshoosh` is a self-hosted SSH/TUI workspace chat for small teams and operators who want real-time communication over SSH instead of a web interface.

- Who it is for: teams that need thread-first collaboration, direct messages, mentions, reactions, unread state, search, and admin workflows without exposing an HTTP app.
- What it is not: an HTTP service or web dashboard. `sshoosh` is a raw SSH/TCP server.

Get started in 2 minutes:

```sh
sshoosh bootstrap-token
cargo run -- serve --host 0.0.0.0 --port 2222
ssh -p 2222 "$USER+TOKEN@127.0.0.1"
```

## Quick links

- [Quick Start](#quick-start)
- [Deployment](#deployment)
- [Connection resilience](#connection-resilience)
- [Configuration](#configuration)
- [Remote DB, failover, and encryption](#remote-db-failover-and-encryption)
- [Backup and export](#backup-and-export)
- [CLI reference](#cli)
- [TUI commands](#tui-commands)
- [Notifications](#notifications)
- [For operators and contributors](#for-operators-and-contributors)

## Table of Contents

- [Quick Start](#quick-start)
  - [Step 1: Start the server](#step-1-start-the-server)
  - [Step 2: Bootstrap first owner](#step-2-bootstrap-first-owner)
  - [Step 3: Connect with SSH](#step-3-connect-with-ssh)
  - [Step 4: Optional seed data](#step-4-optional-seed-data)
- [Deployment](#deployment)
  - [Local or LAN](#local-or-lan)
  - [Expose a local deployment](#expose-a-local-deployment)
    - [ngrok TCP](#ngrok-tcp)
    - [Cloudflare Tunnel TCP](#cloudflare-tunnel-tcp)
    - [Tailscale](#tailscale)
    - [SSH reverse tunnel](#ssh-reverse-tunnel)
  - [VPS + systemd](#vps--systemd)
  - [PaaS or container hosts](#paas-or-container-hosts)
  - [Static or serverless hosts](#static-or-serverless-hosts)
- [Connection resilience](#connection-resilience)
- [Configuration](#configuration)
  - [Required](#required)
  - [Optional and environment alternatives](#optional-and-environment-alternatives)
  - [Production tuning](#production-tuning)
- [Remote DB, failover, and encryption](#remote-db-failover-and-encryption)
- [Backup and export](#backup-and-export)
- [Terminal requirements](#terminal-requirements)
- [CLI](#cli)
  - [Core commands](#core-commands)
  - [Users and access control](#users-and-access-control)
  - [Channels and workspace operations](#channels-and-workspace-operations)
  - [Operational tooling](#operational-tooling)
- [TUI commands](#tui-commands)
- [Notifications](#notifications)
- [For operators and contributors](#for-operators-and-contributors)
  - [Development](#development)
  - [Release binary](#release-binary)
  - [systemd](#systemd)
  - [Verification checklist](#verification-checklist)

## Quick Start

The first run path is: start server, bootstrap owner token, connect via SSH.

### Step 1: Start the server

Run the SSH service on TCP port 2222.

```sh
cargo run -- serve --host 0.0.0.0 --port 2222
```

### Step 2: Bootstrap first owner

Generate a one-time bootstrap token to create the first owner account and initialize `#general`.

```sh
sshoosh bootstrap-token
```

When you connect with this token, unknown keys become first-time users by default owner bootstrap flow.

### Step 3: Connect with SSH

Use `username+TOKEN@host` with the token from bootstrap. Unknown keys without a token are rejected before any account rows are written.

```sh
ssh -p 2222 "$USER+TOKEN@127.0.0.1"
```

`#general` is mandatory for activated users and cannot be left, archived, or made private.

### Step 4: Optional: seed data for demos

For local demos, sign in once over SSH first so the database has a real account and key, then run the seed script:

```sh
./scripts/demo_seed.py --db ./sshoosh.sqlite --reset
```

`--reset` clears workspace data, preserves the most recently active account and its SSH keys, promotes that account to owner, and fills the database with six months of realistic team activity.
Use `--owner <username>` to choose the account explicitly.

## Deployment

`sshoosh` is a raw SSH/TCP server, not an HTTP app. Deploy on a host that can run a long-lived process and expose TCP on the `sshoosh serve` port.

| Target | Good fit | Setup notes |
| --- | --- | --- |
| Local or LAN | Testing, homelab, private network use | Bind `0.0.0.0:2222`, open firewall if needed, and connect by host name or LAN IP. |
| Local plus expose | Temporary sharing from laptop or workstation | Use raw TCP tunnel (`ngrok`, Cloudflare Tunnel arbitrary TCP, Tailscale, SSH reverse tunnel). |
| VPS with systemd | Recommended production path | Install release binary, store state under `/var/lib/sshoosh`, and use [`packaging/sshoosh.service`](packaging/sshoosh.service). |
| PaaS or container host | Works only with raw TCP + persistent storage | Railway TCP Proxy and Fly public TCP services can work. HTTP-only hosts are not suitable. |
| Static or serverless hosts | Usually not a fit | `sshoosh` needs inbound SSH/TCP and persistent process state. |

### Local or LAN

```sh
SSHOOSH_DB=./sshoosh.sqlite \
SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519 \
sshoosh serve --host 0.0.0.0 --port 2222

ssh -p 2222 "$USER@<host-or-lan-ip>"
```

### Expose a local deployment

#### ngrok TCP

```sh
ngrok tcp 2222
ssh -p <ngrok-port> "$USER@<ngrok-host>"
```

#### Cloudflare Tunnel TCP

The server keeps an outbound tunnel open; clients run `cloudflared access tcp` locally and then SSH to local forwarded port.

```sh
cloudflared tunnel --hostname sshoosh.example.com --url tcp://localhost:2222
cloudflared access tcp --hostname sshoosh.example.com --url localhost:9222
ssh -p 9222 "$USER@127.0.0.1"
```

#### Tailscale

Prefer private tailnet access to the machine running `sshoosh`.

```sh
ssh -p 2222 "$USER@<tailscale-machine-name-or-ip>"
tailscale funnel --tcp=<allowed-funnel-port> tcp://localhost:2222
```

#### SSH reverse tunnel

Useful when a remote relay host supports forwarded ports.

```sh
ssh -N -R <public-port>:localhost:2222 user@bastion.example.com
ssh -p <public-port> "$USER@bastion.example.com"
```

### VPS + systemd

Bootstrap the production paths you’ll use, then enable service:

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

Then copy [`packaging/sshoosh.service`](packaging/sshoosh.service) and enable it.

### PaaS or container hosts

On Railway, set a fixed internal port (for example `SSHOOSH_PORT=2222`), mount persistent storage for `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`, enable TCP Proxy to that port, then connect to the generated proxy host/port.

On Fly, use persistent storage (`/data`), set:
`SSHOOSH_DB=/data/sshoosh.sqlite`
`SSHOOSH_SERVER_KEY=/data/sshoosh_server_ed25519`
and configure raw TCP pass-through with no HTTP/TLS handlers or PROXY protocol.

### Static or serverless hosts

These are usually a poor fit: `sshoosh` requires inbound SSH/TCP plus persistent process state and cannot run as request/response-only HTTP code.

## Connection resilience

`sshoosh` runs over SSH/TCP, so clients cannot resume the exact same TCP session after path breaks. Reconnecting is safe: durable chat state is stored in DB and auth returns the same account on reconnect.

For laptop/mobile hotspots, tunnels, or unstable NATs, use OpenSSH keepalives:

```text
Host sshoosh
  HostName sshoosh.example.com
  Port 2222
  User alice
  ServerAliveInterval 30
  ServerAliveCountMax 10
  TCPKeepAlive no
```

`ServerAliveInterval` sends encrypted keepalive packets during idle periods.
`ServerAliveCountMax` controls tolerated unanswered packets.
The example above waits roughly five minutes before SSH exits.
`TCPKeepAlive no` avoids lower-level TCP keepalives that can look like hard failures during temporary route loss.

Auto-retry wrapper example:

```sh
autossh -M 0 \
  -o ServerAliveInterval=30 \
  -o ServerAliveCountMax=3 \
  -o TCPKeepAlive=no \
  -p 2222 alice@sshoosh.example.com
```

`Mosh` is not a direct fit: it bootstraps over SSH and then starts `mosh-server` in a normal shell, while `sshoosh` is the SSH application itself.

## Configuration

All flags can also be set with environment variables.

### Required

```sh
SSHOOSH_DB=/var/lib/sshoosh/sshoosh.sqlite
SSHOOSH_SERVER_KEY=/var/lib/sshoosh/sshoosh_server_ed25519
```

### Optional and environment alternatives

```sh
SSHOOSH_DATABASE_URL=libsql://example.turso.io
SSHOOSH_DATABASE_AUTH_TOKEN=...   # required for authenticated remote URLs
SSHOOSH_NO_MOUSE=false
```

### Production tuning

```sh
SSHOOSH_NODE_ID=sshoosh-1                      # stable id for multi-server deployments
SSHOOSH_ENCRYPTION_KEY=...                     # optional base64url 32-byte key
SSHOOSH_MASTER_LEASE_TTL_SECS=15
SSHOOSH_MASTER_HEARTBEAT_SECS=5
SSHOOSH_HOST=0.0.0.0
SSHOOSH_PORT=2222
SSHOOSH_MAX_CONNECTIONS=256
SSHOOSH_MAX_CONNECTIONS_PER_IP=32
```

`SSHOOSH_DATABASE_URL` can also use `https://` or `file:` URLs.
Keep `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY` on durable storage. If server key changes, SSH clients will warn the host key changed.

## Remote DB, failover, and encryption

`SSHOOSH_DATABASE_URL` supports `libsql://`, `https://`, and `file:`.

- Multiple nodes can share one DB for concurrent SSH sessions and writes through the shared SQLite/libSQL transaction layer.
- Maintenance jobs that must be singleton (for example `encrypt migrate`) still use the `main` lease.
- Use stable `SSHOOSH_NODE_ID` for production clusters.
- If `SSHOOSH_ENCRYPTION_KEY` is set, source content is encrypted at rest with XChaCha20-Poly1305.
- FTS index remains plaintext by design so search continues to work.
- Existing plaintext DBs must be migrated with `sshoosh encrypt migrate`.

## Backup and export

Use this split by intention:

- **Backups**: operational recovery of the underlying database file.
- **Export**: portable archive for sharing/exporting content.

```sh
sshoosh backup /var/backups/sshoosh.sqlite
sshoosh export --format json --out /var/backups/sshoosh.json --include-audit
sshoosh export --format markdown --out /var/backups/sshoosh.md
```

Export includes users, channels, threads, comments, DMs, mentions, reactions, notifications, and optional audit rows.
It is not designed as an import format.

`SSHOOSH_DATABASE_URL` to `libsql://` remains operationally for backups only when provider tooling is available.

## Notifications

V1 creates durable in-app notifications for `@username` mentions, new DMs, and replies to participating threads. Muted threads and muted DMs suppress notifications until mute expiry.

Use the following from TUI or CLI:
`/notification list`, `/notification mentions`, `/notification read`
`sshoosh notifications list --actor alice`, `sshoosh notifications mark-read --actor alice`

Notification rows show source channel/thread/DM. With mouse support, click a source row to open it; public channels auto-join as needed, private still requires membership.
Topbar counters are clickable shortcuts for notification and mention lists.

Terminal notifications are opt-in per account:
`/notification terminal on`, `/notification terminal off`, `/notification terminal status`.

## Terminal requirements

Use an SSH client and terminal with UTF-8 support.
Mouse support, bracketed paste, OSC52 copy, OSC8 hyperlinks, and cursor shape hints improve the experience.
Disable mouse reporting for terminals that mis-handle it:

```sh
sshoosh --no-mouse
# or
SSHOOSH_NO_MOUSE=true
```

## CLI

Protected CLI commands require `--actor <username>` to attribute an action to an active account.

### Core commands

| Command | Purpose |
| --- | --- |
| `sshoosh serve` | Start the SSH/TUI server |
| `sshoosh bootstrap-token` | Create bootstrap token for first-time owner setup |
| `sshoosh doctor` | Run server health checks |
| `sshoosh doctor --repair-search` | Rebuild search metadata |
| `sshoosh invite --role member --ttl-hours 24` | Generate an invite with explicit expiry |
| `sshoosh backup /path/to/backup.sqlite` | Create a local backup copy |
| `sshoosh master status` | Inspect current master lease owner |
| `sshoosh encrypt migrate` | Migrate plaintext DB content to encrypted format |

### Users and access control

| Command | Purpose |
| --- | --- |
| `sshoosh users list` | List users |
| `sshoosh users rename alice alice-prod` | Rename a user |
| `sshoosh users display-name alice "Alice Lee"` | Set display name |
| `sshoosh users disable alice` | Disable a user |
| `sshoosh users role alice admin` | Set user role |
| `sshoosh keys list` | List SSH keys |
| `sshoosh keys add "ssh-ed25519 AAAA..." --username alice --label laptop` | Add SSH key |
| `sshoosh keys label <key-id-or-fingerprint> desktop` | Label SSH key |
| `sshoosh keys revoke <key-id-or-fingerprint>` | Revoke SSH key |
| `sshoosh invites list` | List invites |
| `sshoosh invites create --role admin --ttl-hours 2` | Create invite |
| `sshoosh invites revoke <invite-id>` | Revoke invite |

### Channels and workspace operations

| Command | Purpose |
| --- | --- |
| `sshoosh channels list` | List channels |
| `sshoosh channels create engineering` | Create public channel |
| `sshoosh channels create ops-secret --private` | Create private channel |
| `sshoosh channels join engineering --actor alice` | Join channel |
| `sshoosh channels leave engineering --actor alice` | Leave channel |
| `sshoosh channels rename engineering eng` | Rename channel |
| `sshoosh channels topic eng "Build and release coordination"` | Set channel topic |
| `sshoosh channels archive eng` | Archive channel |
| `sshoosh channels unarchive eng` | Restore archived channel |
| `sshoosh channels members ops-secret` | List channel members |
| `sshoosh channels add-member ops-secret alice` | Add member to private/public channel |
| `sshoosh channels remove-member ops-secret alice` | Remove member from channel |

### Operational tooling

| Command | Purpose |
| --- | --- |
| `sshoosh notifications list --actor alice` | List notifications |
| `sshoosh notifications mark-read --actor alice` | Mark notifications read |
| `sshoosh audit list --limit 100` | Show audit events |
| `sshoosh export --format json --out /path/to/export.json --include-audit` | Export JSON archive |
| `sshoosh export --format markdown --out /path/to/export.md` | Export Markdown archive |

### TUI Commands

Public channels use explicit membership. Use `channels list` to discover available public channels. Content is visible/searchable only after joining.

#### Common commands

| Command | Purpose |
| --- | --- |
| `/invite new [member|admin] [ttl-hours]` | Create invitation |
| `/invite list` | List invites |
| `/invite revoke invite-id` | Revoke invite |
| `/channel new name` | Create public channel |
| `/channel private name` | Create private channel |
| `/channel list` | List channels |
| `/channel join #channel` | Join channel |
| `/channel leave [#channel]` | Leave channel |
| `/channel topic [#channel] topic` | Set channel topic |
| `/channel rename [#channel] name` | Rename channel |
| `/channel archive [#channel]` | Archive channel |
| `/channel unarchive #channel` | Unarchive channel |
| `/thread new title` | Create thread |
| `/thread rename title` | Rename thread |
| `/search query` | Search content |
| `/save index` | Save message/comment |
| `/unsave index` | Unsave message/comment |
| `/more` | Load older content in list |
| `/older` | Load previous page |

#### User, key, and membership commands

| Command | Purpose |
| --- | --- |
| `/user list` | List users |
| `/user profile display-name` | Show profile display name |
| `/user username username` | View/set username |
| `/user disable @user` | Disable user |
| `/user enable @user` | Enable user |
| `/user role @user owner|admin|member` | Set user role |
| `/key list` | List your keys |
| `/key my` | Show current key info |
| `/key add ssh-ed25519... [| label]` | Add SSH key |
| `/key label key-id label` | Label SSH key |
| `/key revoke key-id-or-fingerprint` | Revoke SSH key |
| `/channel members #channel` | List channel members |
| `/channel add #channel @user` | Add member |
| `/channel remove #channel @user` | Remove member |

#### Message and thread lifecycle commands

| Command | Purpose |
| --- | --- |
| `/comment edit index body` | Edit comment |
| `/comment delete index` | Delete comment |
| `/dm open @user` | Open DM conversation |
| `/dm edit index body` | Edit DM message |
| `/dm delete index` | Delete DM message |
| `/thread delete` | Delete thread |
| `/thread archive` | Archive thread |
| `/thread unarchive` | Unarchive thread |
| `/thread pin` | Pin thread |
| `/thread unpin` | Unpin thread |
| `/thread mute [hours]` | Mute thread |
| `/thread unmute` | Unmute thread |
| `/reaction add emoji [comment-or-message-index]` | Add reaction |
| `/reaction remove emoji [comment-or-message-index]` | Remove reaction |

Threads and DMs are marked read when opened in detail view.
Manual unread remains until the item is opened again or explicitly marked read.

In compose mode, `Ctrl-X E` prefills an edit command for your latest comment or latest DM message.
With mouse support enabled, right-click one of your comments or DM messages to open the message menu, then choose edit or delete.
Deletes require confirmation.

Bare URLs and Markdown links render as OSC8 terminal hyperlinks where supported.
`sshoosh` does not open links on the server; use terminal link support or copy visible URL.

## For operators and contributors

### Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Reloadable local development:

```sh
cargo run -- dev --host 127.0.0.1 --port 2222
cargo run -- dev-ssh --host 127.0.0.1 --port 2222
```

### Release binary

Tagged releases build and publish binaries for:
Linux x64, Linux arm64, macOS Intel, macOS Apple Silicon, Windows x64.

```sh
git tag v0.1.0
git push origin v0.1.0
```

Local install:

```sh
cargo build --release
install -m 0755 target/release/sshoosh /usr/local/bin/sshoosh
```

Each artifact contains a single `sshoosh` binary.
Runtime state is the SQLite database and SSH host key from `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`.

### systemd

Copy [`packaging/sshoosh.service`](packaging/sshoosh.service) to `/etc/systemd/system/sshoosh.service`, adjust binary path if needed.

```sh
sudo install -d -o sshoosh -g sshoosh /var/lib/sshoosh
sudo systemctl daemon-reload
sudo systemctl enable --now sshoosh
```

### Verification checklist

After first setup, verify these flows:

1. Start service:

```sh
sshoosh bootstrap-token
sshoosh serve --host 0.0.0.0 --port 2222
```

2. SSH connect with token and confirm:
`#general` auto-created, user activated, explicit membership behavior works.

3. Run admin path checks:

```sh
sshoosh doctor
sshoosh doctor --repair-search
sshoosh master status
```

4. Validate persistence:

```sh
sshoosh backup /var/backups/sshoosh.sqlite
sshoosh export --format json --out /var/backups/sshoosh.json --include-audit
```
