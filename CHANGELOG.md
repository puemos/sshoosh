# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
