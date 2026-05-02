<p align="center">
  <picture>
    <source
      media="(prefers-color-scheme: dark)"
      srcset="docs/public/assets/sshoosh-logo.svg"
    >
    <source
      media="(prefers-color-scheme: light)"
      srcset="docs/public/assets/sshoosh-logo-light.svg"
    >
    <img
      src="docs/public/assets/sshoosh-logo-light.svg"
      alt="sshoosh logo"
      width="420"
    >
  </picture>
</p>

# sshoosh

`sshoosh` is a self-hosted SSH/TUI workspace chat for small teams and operators who want real-time collaboration over SSH.

- Who it is for: teams that want direct messages, mentions, reactions, unread state, search, and admin workflows without exposing an HTTP app.
- What it is not: an HTTP service or web dashboard. `sshoosh` is a raw SSH/TCP server.

## Quick start

1. Install the release binary.

   ```sh
   curl -fsSL https://raw.githubusercontent.com/puemos/sshoosh/main/install.sh | sh
   ```

2. Generate an owner bootstrap token.

   ```sh
   sshoosh bootstrap-token
   ```

3. Start the server.

   ```sh
   SSHOOSH_DB=./sshoosh.sqlite \
   SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519 \
   sshoosh serve --host 0.0.0.0 --port 2222
   ```

4. Connect with SSH.

   ```sh
   ssh -p 2222 "$USER+TOKEN@127.0.0.1"
   ```

## Docs and reference

The README is intentionally compact. For complete deployment, configuration, and command details, use the docs site:

- https://puemos.github.io/sshoosh/

If you need just the essentials, see the sections below.

## Deployment summary

| Setup path            | Recommendation             | Notes |
| --------------------- | -------------------------- | ----- |
| Local or LAN          | `0.0.0.0:2222` on private network | Bind to your host IP and keep firewall rules tight. |
| Temporary sharing     | Tunnel `sshoosh` TCP port   | Works with ngrok, Cloudflare Tunnel, Tailscale, or SSH reverse tunnels. |
| Production            | VPS + systemd              | Use durable storage for `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY`. |
| Docker                | GHCR image + persistent volume | Use `ghcr.io/puemos/sshoosh` and expose raw TCP port 2222. |
| PaaS/container hosts  | Use only raw TCP paths       | Avoid HTTP-only hosts. |

Docker quick start:

```sh
docker volume create sshoosh-data
docker run --rm -v sshoosh-data:/data ghcr.io/puemos/sshoosh:latest bootstrap-token
docker run -d --name sshoosh --restart unless-stopped \
  -p 2222:2222 \
  -v sshoosh-data:/data \
  ghcr.io/puemos/sshoosh:latest
```

## Core commands

```sh
sshoosh bootstrap-token
sshoosh serve
sshoosh doctor
sshoosh doctor --repair-search
sshoosh backup /var/backups/sshoosh.sqlite
sshoosh export --format json --out /var/backups/sshoosh.json --include-audit
```

Use the full command reference in the docs for CLI/TUI details.

## Environment variables (quick starter)

```sh
SSHOOSH_DB=/var/lib/sshoosh/sshoosh.sqlite
SSHOOSH_SERVER_KEY=/var/lib/sshoosh/sshoosh_server_ed25519
SSHOOSH_HOST=0.0.0.0
SSHOOSH_PORT=2222
```

For advanced configuration options and production tuning, see the docs site.

## Notes

- `#general` is mandatory for activated users.
- Reconnect behavior is resilient by design with stable account identity in DB.
