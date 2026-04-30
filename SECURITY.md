# Security

Report security issues privately to the project owner or maintainers. Do not open public issues for vulnerabilities until a fix is available.

## Supported Scope

The V1 security boundary is the SSH server, SQLite database, account/key lifecycle, channel visibility, notifications, and export paths in the current `main` branch.

## Operational Guidance

- Protect `SSHOOSH_DB` and `SSHOOSH_SERVER_KEY` with filesystem permissions.
- Treat exports and SQLite backups as sensitive data.
- Rotate or revoke SSH keys with `sshoosh keys revoke` when a key is lost.
- Run behind a firewall or restricted network policy when possible.

## Reporting Contents

Include the affected version or commit, reproduction steps, expected impact, and any relevant logs with secrets removed.
