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

## Demo

<p align="center">
  <video src="https://github.com/user-attachments/assets/3ed979ae-2ba2-4514-afd2-a38e5d62237f" width="420" controls/>
</p>

## Quick start

1. Install the release binary.

```sh
curl -fsSL https://raw.githubusercontent.com/puemos/sshoosh/main/install.sh | sh
```

Or install with Homebrew:

```sh
brew install puemos/tap/sshoosh
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

4. Connect with SSH and paste the token at the masked `Token:` prompt.

   ```sh
   ssh -p 2222 127.0.0.1
   ```

   Your SSH key is registered the first time you redeem a bootstrap, invite,
   or device link token. The token is requested over keyboard-interactive auth so it never
   appears in the SSH user field, `ps`, sshd logs, or shell history.
   For bootstrap and invite tokens, sshoosh then asks you to choose your
   username in the TUI. Device link tokens sign in to the existing account.
   Once your key is bound, future connections skip the prompt.

## Docs and reference

For complete deployment, configuration, and command details, read the docs site:

- https://puemos.github.io/sshoosh/


## Deployment summary

| Setup path            | Recommendation             | Notes |
| --------------------- | -------------------------- | ----- |
| Local or LAN          | `0.0.0.0:2222` on private network | Bind to your host IP and keep firewall rules tight. |
| Temporary sharing     | Tunnel `sshoosh` TCP port   | Works with ngrok, Cloudflare Tunnel, Tailscale, or SSH reverse tunnels. |
| Production            | VPS + systemd              | Use `sudo sshoosh daemon install` so the service runs as the locked-down `sshoosh` user. |
| Docker                | GHCR image + persistent volume | Run as the image's non-root user, persist `/data`, and drop container capabilities. |
| PaaS/container hosts  | Use only raw TCP paths       | Avoid HTTP-only hosts. |

VPS quick path:

```sh
curl -fsSL https://raw.githubusercontent.com/puemos/sshoosh/main/install.sh | sudo sh -s -- --dir /usr/local/bin
sudo /usr/local/bin/sshoosh daemon install --binary /usr/local/bin/sshoosh
sudo sh -c 'set -a; . /etc/sshoosh/sshoosh.env; set +a; exec sudo -E -u sshoosh /usr/local/bin/sshoosh bootstrap-token'
ssh -p 2222 sshoosh.example.com
# Paste the bootstrap token at the masked "Token:" prompt.
```

To apply a new release on a daemon install, replace the binary in place and restart the managed service:

```sh
curl -fsSL https://raw.githubusercontent.com/puemos/sshoosh/main/install.sh \
  | sudo sh -s -- --dir /usr/local/bin --version vX.Y.Z
sudo /usr/local/bin/sshoosh daemon restart --backup
```

Docker quick start:

```sh
docker volume create sshoosh-data
docker run --rm -v sshoosh-data:/data ghcr.io/puemos/sshoosh:latest bootstrap-token
docker run -d --name sshoosh --restart unless-stopped \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
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
