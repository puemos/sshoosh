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
  cargo test

# Build a release artifact.
build:
  cargo build --release

# Run the full CI-equivalent validation locally.
ci:
  just format lint test build

# Run every local quality gate including formatting, lint, tests, and docs?
all: ci
