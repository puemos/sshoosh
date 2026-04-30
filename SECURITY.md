# Security

Report security issues privately to the project owner or maintainers. Do not open public issues for vulnerabilities until a fix is available.

## Supported Scope

The V1 security boundary is the SSH server, SQLite database, account/key lifecycle, channel visibility, notifications, and export paths in the current `main` branch.

## Operational Guidance

- Protect `SSHOOSH_DB`, `SSHOOSH_DATABASE_AUTH_TOKEN`, `SSHOOSH_ENCRYPTION_KEY`, and `SSHOOSH_SERVER_KEY`.
- Treat exports, SQLite backups, remote database backups, and provider snapshots as sensitive data.
- App-level encryption protects source content fields when `SSHOOSH_ENCRYPTION_KEY` is set, but `search_index` remains plaintext by design so full-text search continues to work.
- Use provider encryption or encrypted disks in addition to app-level encryption when storage-level protection is required.
- Use stable `SSHOOSH_NODE_ID` values for multi-server deployments sharing one remote database.
- Rotate or revoke SSH keys with `sshoosh keys revoke` when a key is lost.
- Run behind a firewall or restricted network policy when possible.

## Reporting Contents

Include the affected version or commit, reproduction steps, expected impact, and any relevant logs with secrets removed.
