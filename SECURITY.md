# Security

Report security issues privately to the project owner or maintainers. Do not open public issues for vulnerabilities until a fix is available.

## Supported Scope

The V1 security boundary is the SSH server, SQLite database, account/key lifecycle, channel visibility, notifications, and export paths in the current `main` branch.

## Operational Guidance

- `sshoosh` is its own SSH server, not a shell wrapper around system `sshd`. It accepts the TUI session path only; command execution, subsystems such as SFTP, agent/X11 forwarding, TCP forwarding, and streamlocal forwarding are not supported.
- For VPS production installs, prefer `sudo sshoosh daemon install` over a hand-written service. The generated Linux systemd unit runs as the dedicated `sshoosh` user, keeps state owner-only, drops capabilities, and restricts writable paths, devices, kernel surfaces, namespaces, and address families.
- For Docker on a VPS, persist `/data`, keep the published raw TCP port behind a firewall, VPN, or IP allowlist where possible, and run with `--cap-drop=ALL --security-opt no-new-privileges`.
- Protect `SSHOOSH_DB`, `SSHOOSH_DATABASE_AUTH_TOKEN`, `SSHOOSH_ENCRYPTION_KEY`, and `SSHOOSH_SERVER_KEY`.
- Treat exports, SQLite backups, remote database backups, and provider snapshots as sensitive data.
- App-level encryption protects source content fields when `SSHOOSH_ENCRYPTION_KEY` is set, but `search_index` remains plaintext by design so full-text search continues to work.
- Use provider encryption or encrypted disks in addition to app-level encryption when storage-level protection is required.
- Use stable `SSHOOSH_NODE_ID` values for multi-server deployments sharing one remote database.
- Rotate or revoke SSH keys with `sshoosh keys revoke` when a key is lost.
- Run behind a firewall or restricted network policy when possible.

## Reporting Contents

Include the affected version or commit, reproduction steps, expected impact, and any relevant logs with secrets removed.
