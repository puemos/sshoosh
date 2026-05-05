#!/bin/sh
set -eu

repo="${SSHOOSH_INSTALL_REPO:-puemos/sshoosh}"
version="${SSHOOSH_INSTALL_VERSION:-}"
install_dir="${SSHOOSH_INSTALL_DIR:-}"
target="${SSHOOSH_INSTALL_TARGET:-}"
raw_url="${SSHOOSH_INSTALL_SCRIPT_URL:-https://raw.githubusercontent.com/${repo}/main/install.sh}"

usage() {
    cat <<EOF
Install sshoosh from a GitHub release.

Usage:
  install.sh [--dir DIR] [--version vX.Y.Z] [--target TARGET]

Environment:
  SSHOOSH_INSTALL_DIR       Install directory. Defaults to \$HOME/.local/bin, or /usr/local/bin as root.
  SSHOOSH_INSTALL_VERSION   Release tag to install. Defaults to the latest GitHub release.
  SSHOOSH_INSTALL_TARGET    Release target triple override.
  SSHOOSH_INSTALL_REPO      GitHub repo override. Defaults to puemos/sshoosh.
EOF
}

die() {
    echo "install.sh: $*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dir)
            [ "$#" -ge 2 ] || die "--dir requires a value"
            install_dir="$2"
            shift 2
            ;;
        --dir=*)
            install_dir="${1#--dir=}"
            shift
            ;;
        --version)
            [ "$#" -ge 2 ] || die "--version requires a value"
            version="$2"
            shift 2
            ;;
        --version=*)
            version="${1#--version=}"
            shift
            ;;
        --target)
            [ "$#" -ge 2 ] || die "--target requires a value"
            target="$2"
            shift 2
            ;;
        --target=*)
            target="${1#--target=}"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

need curl
need tar
need mktemp
need sed
need grep

if [ -z "$version" ]; then
    latest_json="$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest")" ||
        die "could not fetch latest release metadata for ${repo}"
    version="$(printf '%s\n' "$latest_json" |
        sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
        sed -n '1p')"
    [ -n "$version" ] || die "could not determine latest release tag"
fi

if [ -z "$target" ]; then
    os="$(uname -s)"
    arch="$(uname -m)"
    case "${os}:${arch}" in
        Linux:x86_64|Linux:amd64)
            target="x86_64-unknown-linux-gnu"
            ;;
        Linux:aarch64|Linux:arm64)
            target="aarch64-unknown-linux-gnu"
            ;;
        Darwin:x86_64|Darwin:amd64)
            target="x86_64-apple-darwin"
            ;;
        Darwin:arm64|Darwin:aarch64)
            target="aarch64-apple-darwin"
            ;;
        *)
            die "unsupported platform: ${os} ${arch}; set SSHOOSH_INSTALL_TARGET to override"
            ;;
    esac
fi

if [ -z "$install_dir" ]; then
    if [ "$(id -u)" = "0" ]; then
        install_dir="/usr/local/bin"
    else
        [ -n "${HOME:-}" ] || die "HOME is not set; use --dir or SSHOOSH_INSTALL_DIR"
        install_dir="${HOME}/.local/bin"
    fi
fi

archive="sshoosh-${version}-${target}.tar.gz"
release_url="https://github.com/${repo}/releases/download/${version}"
tmp_dir="$(mktemp -d)"

cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

echo "Installing sshoosh ${version} for ${target}"
curl -fL "${release_url}/${archive}" -o "${tmp_dir}/${archive}" ||
    die "could not download ${archive}"
curl -fL "${release_url}/SHA256SUMS.txt" -o "${tmp_dir}/SHA256SUMS.txt" ||
    die "could not download SHA256SUMS.txt"

grep " ${archive}\$" "${tmp_dir}/SHA256SUMS.txt" > "${tmp_dir}/SHA256SUMS.selected" ||
    die "checksum entry not found for ${archive}"

if command -v sha256sum >/dev/null 2>&1; then
    (cd "$tmp_dir" && sha256sum -c SHA256SUMS.selected) ||
        die "checksum verification failed"
elif command -v shasum >/dev/null 2>&1; then
    (cd "$tmp_dir" && shasum -a 256 -c SHA256SUMS.selected) ||
        die "checksum verification failed"
else
    die "missing checksum tool: install sha256sum or shasum"
fi

tar -xzf "${tmp_dir}/${archive}" -C "$tmp_dir" ||
    die "could not extract ${archive}"
binary="${tmp_dir}/sshoosh-${version}-${target}/sshoosh"
[ -f "$binary" ] || die "release archive did not contain sshoosh binary"

if ! mkdir -p "$install_dir" 2>/dev/null; then
    echo "install.sh: could not create ${install_dir}" >&2
    echo "Run with an explicit elevated install, for example:" >&2
    echo "  curl -fsSL ${raw_url} | sudo sh -s -- --version ${version} --dir ${install_dir}" >&2
    exit 1
fi

if [ ! -w "$install_dir" ]; then
    echo "install.sh: ${install_dir} is not writable" >&2
    echo "Run with an explicit elevated install, for example:" >&2
    echo "  curl -fsSL ${raw_url} | sudo sh -s -- --version ${version} --dir ${install_dir}" >&2
    exit 1
fi

install -m 0755 "$binary" "${install_dir}/sshoosh" ||
    die "could not install sshoosh to ${install_dir}"

echo "Installed ${install_dir}/sshoosh"
if ! command -v sshoosh >/dev/null 2>&1 && [ "${install_dir}" = "${HOME:-}/.local/bin" ]; then
    echo "Add ${install_dir} to PATH if your shell cannot find sshoosh:"
    echo "  export PATH=\"${install_dir}:\$PATH\""
fi

cat <<EOF

Next:
  sshoosh bootstrap-token
  SSHOOSH_DB=./sshoosh.sqlite SSHOOSH_SERVER_KEY=./sshoosh_server_ed25519 sshoosh serve --host 0.0.0.0 --port 2222
  ssh -p 2222 127.0.0.1
  # Paste the bootstrap token at the masked "Token:" prompt.
EOF
