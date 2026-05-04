set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Format code and fail if formatting differs.
format:
  cargo fmt --check

# Apply rustfmt.
fmt:
  cargo fmt

# Lint with clippy and treat all warnings as errors.
lint:
  cargo clippy --all-targets -- -D warnings

# Run the full test suite.
test:
  cargo test -- --test-threads=1

# Run the Linux container daemon install/uninstall e2e.
daemon-e2e:
  SSHOOSH_RUN_DAEMON_E2E=1 cargo test --test daemon_e2e -- --nocapture

# Build a release artifact.
build:
  cargo build --release

# Run the full CI-equivalent validation locally.
ci:
  just format lint test build

# Run every local quality gate including formatting, lint, tests, and docs?
all: ci
