# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

### Removed

## [0.1.2] - 2026-05-02

### Changed
- Bootstrap and invite tokens are now redeemed at an SSH keyboard-interactive `Invite token:` prompt instead of being mashed into the SSH user field. Tokens no longer appear in `ps`, sshd auth logs, terminal scrollback, or shell history.

### Removed
- Removed the legacy `username+TOKEN@host` connection string. New SSH keys must redeem their token at the keyboard-interactive prompt.

## [0.1.1] - 2026-05-02

### Fixed
- Corrected a duplicate bind parameter in DM sidebar loading that could return incorrect rows.
- Upgraded SSH session startup failure logs from debug to warn for better operational visibility.
- Made SSH authentication robust to account bootstrap/load failures by returning a clear rejection and preventing session failures.

## [0.1.0] - 2026-05-02

### Added
- Homebrew tap formula backed by release binaries
- Install script for easy local and server setup
- Incremental list pagination for channels and threads
- Archived notification management
- Saved messages rail with visible count and navigation
- Emoji reactions with chip display and add affordance
- Inline command search in composer

### Fixed
- Enforced lease and hardened failure paths for reliability
- Multiline compose input preservation
- Saved result click navigation and title clarity
- Pinned thread marker legibility
- Peer visibility in direct messages
- Message-scoped text selection on copy
- Thread scroll position under cursor
- Reduced noisy toast confirmations
- Aligned saved results with notification rail

### Performance
- Scaled sidebar and presence scan queries
- Improved snapshot scalability for large workspaces

### Security
- Prevented account takeover and sensitive data leaks
- Prevented private data exposure and unsafe key storage
- Enforced SSH admission limits and hardened key creation

## [0.1.1] - 2026-05-02

### Fixed
- Corrected a duplicate bind parameter in DM sidebar loading that could return incorrect rows.
- Upgraded SSH session startup failure logs from debug to warn for better operational visibility.
- Made SSH authentication robust to account bootstrap/load failures by returning a clear rejection and preventing session failures.
