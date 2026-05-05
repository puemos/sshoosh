---
title: Overview
description: Install, run, and operate sshoosh, the SSH-native TUI workspace chat.
---

`sshoosh` is a self-hosted SSH/TUI workspace chat. Users connect with an SSH key and get a terminal UI for explicit-membership channels, thread-first discussions, direct messages, notifications, mentions, reactions, unread state, full-text search, presence, export, and administration.

## Quick Start

Install the release binary, run the server, and connect with any SSH client:

```sh
curl -fsSL https://raw.githubusercontent.com/puemos/sshoosh/main/install.sh | sh
sshoosh bootstrap-token
SSHOOSH_DB=./sshoosh.sqlite \
SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519 \
sshoosh serve --host 0.0.0.0 --port 2222
ssh -p 2222 127.0.0.1
# Paste the bootstrap token at the "Token:" prompt,
# then choose your username in the TUI.
```

The installer downloads the matching GitHub release binary, verifies it against `SHA256SUMS.txt`, and installs only the `sshoosh` executable. It does not create users, write systemd units, or start services. Use `install.sh --dir DIR --version vX.Y.Z` when you need an explicit install directory or release tag.

Homebrew also installs the same executable-only package:

```sh
brew install puemos/tap/sshoosh
```

Connect to the host and paste the one-time bootstrap token at the masked `Token:` prompt. After the token is accepted, sshoosh opens a setup modal where you choose the first owner's username, then creates `#general` and auto-joins the owner to it. New users invited later follow the same flow with their invite token and choose their username in the TUI. Already signed-in users can run `/key link [label]` to create a 10-minute device link token for a new SSH key on the same account. Owners and admins can also pre-register a key with `sshoosh keys add`, in which case no prompt appears. Unknown keys that do not redeem a token are rejected before any account rows are written. `#general` is mandatory for activated users and cannot be left, archived, or made private.

The `Token:` prompt is delivered over SSH keyboard-interactive auth (RFC 4256) with input masking, so bootstrap, invite, and device link tokens never appear in the SSH user field, `ps`, sshd logs, terminal scrollback, or shell history.

## Add another device

Each account can have multiple SSH keys. If you are already signed in on one device, create a short-lived link token from inside the TUI:

```text
/key link laptop
```

The optional label is stored on the new key so `/key my` stays readable. Copy the token from the modal, then connect from the new device with the SSH key you want to add:

```sh
ssh -p 2222 host
```

Paste the device link token at the masked `Token:` prompt. `sshoosh` links the offered SSH public key to your existing account, marks the token used, and signs you in as the same user. The SSH username is not used to choose the account during device linking; the token owner is.

Device link tokens are bearer secrets. They expire after 10 minutes, are single-use, and are stored only as hashes. If a token expires or is lost, run `/key link [label]` again from an already linked device. Owners and admins can still pre-register a user's public key with `sshoosh keys add` when a self-service handoff is not practical.

## Quick Deploy

`sshoosh` is a raw SSH/TCP server, not an HTTP app. Deploy it on a host that can run a long-lived process and expose TCP to the port where `sshoosh serve` listens. Keep `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY` on persistent storage; the optional `SSHOOSH_ENCRYPTION_KEY` must also be stable if encryption is enabled. Losing the server key makes SSH clients warn that the host key changed.

| Target | Good fit | Setup notes |
| --- | --- | --- |
| Local or LAN | Testing, homelab, private network use | Bind `0.0.0.0:2222`, open the firewall if needed, and connect by host name or LAN IP. |
| Local plus expose | Temporary sharing from a laptop or workstation | Use a raw TCP tunnel such as [ngrok TCP](https://ngrok.com/docs/universal-gateway/tcp), [Cloudflare Tunnel arbitrary TCP](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/non-http/cloudflared-authentication/arbitrary-tcp/), Tailscale, or an SSH reverse tunnel. |
| VPS with systemd | Recommended production path | Install the release binary, then run `sudo sshoosh daemon install` to create the service user, state directory, env file, and service unit. |
| Docker | Lightweight container path | Use `ghcr.io/puemos/sshoosh`, publish raw TCP port 2222, and keep `/data` on persistent storage. |
| PaaS or container host | Works only with raw TCP and persistent storage | Railway TCP Proxy and Fly public TCP services can fit. HTTP-only app hosts need a raw TCP feature. |
| Static or serverless hosts | Usually not a fit | `sshoosh` needs inbound SSH/TCP and process state, not request/response HTTP execution. |

Local or LAN:

```sh
SSHOOSH_DB=./sshoosh.sqlite \
SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519 \
sshoosh serve --host 0.0.0.0 --port 2222

ssh -p 2222 <host-or-lan-ip>
```

Expose a local server with ngrok TCP:

```sh
ngrok tcp 2222
ssh -p <ngrok-port> <ngrok-host>
```

Expose through Cloudflare Tunnel TCP. The server keeps an outbound tunnel open; clients run `cloudflared access tcp` locally and then SSH to the local forwarded port:

```sh
cloudflared tunnel --hostname sshoosh.example.com --url tcp://localhost:2222
cloudflared access tcp --hostname sshoosh.example.com --url localhost:9222
ssh -p 9222 127.0.0.1
```

For Tailscale, prefer private tailnet access to the machine running `sshoosh`. Tailscale Funnel can be used only when its allowed public TCP ports and TLS behavior fit your client path:

```sh
ssh -p 2222 <tailscale-machine-name-or-ip>
tailscale funnel --tcp=<allowed-funnel-port> tcp://localhost:2222
```

An SSH reverse tunnel is useful when you control a relay host that allows remote forwarded ports:

```sh
ssh -N -R <public-port>:localhost:2222 user@bastion.example.com
ssh -p <public-port> bastion.example.com
```

For a VPS, install the binary and then let `sshoosh` install the production daemon. The daemon command creates the dedicated service account, locked state/config paths, and a systemd unit that runs without extra Linux capabilities and restricts filesystem, device, kernel, namespace, and address-family access. Provision a Linux VM with a public IPv4 (SSH cannot be proxied through HTTP/HTTPS edges), open the host SSH port and the sshoosh port, and block everything else:

```sh
sudo apt update && sudo apt -y install ufw
sudo ufw allow 22/tcp
sudo ufw allow 2222/tcp
sudo ufw --force enable
```

Install the binary, install the daemon, then mint the one-time bootstrap token:

```sh
curl -fsSL https://raw.githubusercontent.com/puemos/sshoosh/main/install.sh | sudo sh -s -- --dir /usr/local/bin
sudo /usr/local/bin/sshoosh daemon install --binary /usr/local/bin/sshoosh
sudo sh -c 'set -a; . /etc/sshoosh/sshoosh.env; set +a; exec sudo -E -u sshoosh /usr/local/bin/sshoosh bootstrap-token'
```

For a friendly hostname, point an A record at the VM's IPv4. On Cloudflare DNS the record must be **DNS only** (gray cloud); the proxied (orange cloud) mode does not pass raw SSH/TCP and will break connections.

```sh
ssh -p 2222 sshoosh.example.com
```

Paste the printed bootstrap token at the masked `Token:` prompt and choose your username in the TUI. Subsequent connections from the same SSH key skip the token prompt.

Docker:

```sh
docker volume create sshoosh-data
docker run --rm -v sshoosh-data:/data ghcr.io/puemos/sshoosh:latest bootstrap-token
docker run -d --name sshoosh --restart unless-stopped \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
  -p 2222:2222 \
  -v sshoosh-data:/data \
  ghcr.io/puemos/sshoosh:latest

ssh -p 2222 127.0.0.1
# Paste the bootstrap token at the "Token:" prompt,
# then choose your username in the TUI.
```

The image runs as a non-root `sshoosh` user, listens on `0.0.0.0:2222`, and stores the SQLite database plus SSH host key under `/data`. Named Docker volumes inherit the image permissions automatically; for bind mounts, make the directory writable by UID/GID `10001`. Keep the published TCP port behind a host firewall, provider firewall, VPN, or IP allowlist when the VPS is internet-facing.

On Railway, set a fixed internal port such as `SSHOOSH_PORT=2222`, mount persistent storage for `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`, then enable TCP Proxy to that port and connect to the generated proxy host and port. On Fly, use a persistent volume such as `/data`, set `SSHOOSH_DB=/data/sshoosh.sqlite` and `SSHOOSH_SERVER_KEY=/data/sshoosh_server_ed25519`, and configure a raw TCP pass-through service with no HTTP/TLS handlers or PROXY protocol in front of `sshoosh`.

## Connection Resilience

`sshoosh` runs over SSH/TCP, so a client cannot resume the exact same TCP session after the network path is broken. Reconnecting is safe: durable chat state lives in the database, and the client will return to the current account after authentication. Unsaved text in the local compose box may be lost if the terminal disconnects.

For laptops, mobile hotspots, tunnels, or NATs that briefly stop passing traffic, configure OpenSSH protocol keepalives on the client. This keeps idle sessions from being closed too aggressively and gives SSH a longer window before it decides the server is unreachable:

```text
Host sshoosh
  HostName sshoosh.example.com
  Port 2222
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
  -p 2222 sshoosh.example.com
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

Use `--no-mouse` or `SSHOOSH_NO_MOUSE=true` if your terminal has problematic mouse reporting. UTF-8 support is required. Mouse support, bracketed paste, OSC52 copy, OSC8 hyperlinks, and cursor shape hints improve the experience but are optional.

For production, prefer a VPN, private network, or firewall allowlist in front of the SSH port. The in-app limits above protect the process from common brute-force and connection-abuse patterns, but provider firewalls or upstream DDoS controls are still required for volumetric attacks. `sshoosh` emits structured security logs such as `auth_failed`, `token_redeem_failed`, `connection_rejected`, and `auth_penalty_applied`; those messages are intended to be usable from tools such as fail2ban without exposing plaintext tokens.

Example fail2ban filter pattern:

```text
failregex = .*(auth_failed|token_redeem_failed|connection_rejected|auth_penalty_applied).*peer_ip=<HOST>.*
ignoreregex =
```

## Commands

### Core CLI commands

```sh
sshoosh serve
sshoosh bootstrap-token
sshoosh doctor
sshoosh doctor --repair-search
sshoosh backup /path/to/backup.sqlite
sshoosh master status
sshoosh encrypt migrate
sshoosh daemon install
sshoosh daemon uninstall
sshoosh audit list --limit 100
sshoosh notifications list --actor alice
sshoosh notifications mark-read --actor alice
```

Protected CLI commands require `--actor <owner-or-admin>` to attribute the action to a specific active account.

| Area | CLI examples |
| --- | --- |
| Users | `sshoosh users list`, `sshoosh users rename alice alice-prod`, `sshoosh users display-name alice "Alice Lee"`, `sshoosh users disable alice`, `sshoosh users enable alice`, `sshoosh users role alice admin` |
| Keys | `sshoosh keys list`, `sshoosh keys my`, `sshoosh keys add "ssh-ed25519 AAAA..." --username alice`, `sshoosh keys label <key-id-or-fingerprint> laptop`, `sshoosh keys revoke <key-id-or-fingerprint>` |
| Invites | `sshoosh invites create --role admin --ttl-hours 2`, `sshoosh invites revoke <invite-id>` |
| Channels | `sshoosh channels list`, `sshoosh channels create engineering`, `sshoosh channels create ops-secret --private`, `sshoosh channels join engineering`, `sshoosh channels leave engineering`, `sshoosh channels rename engineering eng`, `sshoosh channels topic eng "Build notes"`, `sshoosh channels archive eng`, `sshoosh channels unarchive eng`, `sshoosh channels members ops-secret`, `sshoosh channels add-member ops-secret alice`, `sshoosh channels remove-member ops-secret alice` |
| Notifications | `sshoosh notifications list --actor alice`, `sshoosh notifications mark-read --actor alice` |
| Encryption | `sshoosh encrypt migrate` |
| Master | `sshoosh master status` |
| Daemon | `sshoosh daemon install`, `sshoosh daemon uninstall --purge-data`, `sshoosh daemon install --backend systemd --dry-run` |
| Audit | `sshoosh audit list --limit 100` |
| Export | `sshoosh export --format json --out export.json --include-audit`, `sshoosh export --format markdown --out export.md` |
| Backup | `sshoosh backup /path/to/backup.sqlite` |

### Developer CLI commands

These commands are intended for local workflows and local testing:

```sh
sshoosh dev --host 127.0.0.1 --port 2222
sshoosh dev-ssh --host 127.0.0.1 --port 2222
sshoosh dev-db-bench --users 50 --channels 50 --threads 1000 --comments 100000 --dms 5000
```

### Complete TUI command reference

```text
/invite new [member|admin] [ttl-hours]
/invite list
/invite revoke invite-id
/channel new name
/channel private name
/channel list
/channel join #channel
/channel leave [#channel]
/channel topic #channel <topic>
/channel rename #channel <name>
/channel archive [#channel]
/channel unarchive #channel
/channel members #channel
/channel add #channel @user
/channel remove #channel @user
/thread new title
/thread rename title
/thread delete
/thread archive
/thread unarchive
/thread pin
/thread unpin
/thread mute [hours]
/thread unmute
/thread read
/thread unread
/dm open @user
/dm edit index body
/dm delete index
/dm mute [hours]
/dm unmute
/dm read
/dm unread
/user list
/user profile display-name
/user username new-name
/user disable @user
/user enable @user
/user role @user owner|admin|member
/key list
/key my
/key add ssh-ed25519... [label]
/key link [label]
/key label key-id-or-fingerprint label
/key revoke key-id-or-fingerprint
/search query
/label $label
/comment edit index body
/comment delete index
/notification mentions
/notification list
/notification read
/notification terminal on
/notification terminal off
/notification terminal status
/audit
/audit list
/reaction add emoji [comment-or-message-index]
/reaction remove emoji [comment-or-message-index]
/reaction delete emoji [comment-or-message-index]
/save index
/unsave index
/more
/older
/help
/quit
``` 

Command aliases are available in the TUI: `/chan` (`/channel`), `/t` (`/thread`), `/d` (`/dm`), `/msg` (`/dm`), `/tag` (`/label`), and `/q` (`/quit`).

Use `/older` to load older thread or DM message history. Use `/more` to load additional search, saved, and notification rows.

### Readability note

`/audit` is shorthand for `/audit list`.

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

## Backup, Export, And Daemons

Use SQLite backups for operational recovery and exports for portable archives:

```sh
sshoosh backup /var/backups/sshoosh.sqlite
sshoosh export --format json --out /var/backups/sshoosh.json --include-audit
sshoosh export --format markdown --out /var/backups/sshoosh.md
```

The release artifact is a single `sshoosh` binary. Runtime state is just the SQLite database and SSH host key configured by `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`.

For remote libSQL/Turso, set `SSHOOSH_DATABASE_URL` and `SSHOOSH_DATABASE_AUTH_TOKEN`; this overrides `SSHOOSH_DB`. Multiple servers may share the same database, and all nodes accept SSH sessions and writes through the shared SQLite/libSQL transaction layer. They still contend for the `main` master lease for singleton maintenance commands such as encryption migration. Use stable `SSHOOSH_NODE_ID` values in production.

Set `SSHOOSH_ENCRYPTION_KEY` to a base64url 32-byte key to encrypt source content fields. Run `sshoosh encrypt migrate` once for existing plaintext rows. Search index columns intentionally remain plaintext so FTS works, which means search data remains sensitive at rest.

For production daemon deployments, prefer the built-in service manager installer:

```sh
sudo sshoosh daemon install --binary /usr/local/bin/sshoosh
sudo sshoosh daemon uninstall
```

On Linux, `daemon install` writes `/etc/systemd/system/sshoosh.service`, `/etc/sshoosh/sshoosh.env`, and `/var/lib/sshoosh`. The systemd unit runs as the dedicated `sshoosh` user, uses owner-only state permissions, limits memory/tasks/open files, drops capabilities, and restricts writable paths, devices, kernel surfaces, namespaces, and address families while keeping `/var/lib/sshoosh` writable. On macOS, it writes a root LaunchDaemon under `/Library/LaunchDaemons` and keeps runtime state under `/var/lib/sshoosh`. Generated env files are not embedded into systemd units or launchd plists. Uninstall preserves the database and SSH host key unless `--purge-data` is provided.

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
