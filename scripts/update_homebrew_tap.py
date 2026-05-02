#!/usr/bin/env python3
"""Update puemos/homebrew-tap for a tagged sshoosh release."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


TARGETS = (
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
)

FORMULA_ROW = "| [sshoosh](Formula/sshoosh.rb) | Self-hosted SSH/TUI workspace chat |"
INSTALL_LINE = "`brew install puemos/tap/sshoosh`"
BREWFILE_LINE = 'brew "sshoosh"'


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tap", required=True, help="Path to the checked-out homebrew-tap repository")
    parser.add_argument("--checksums", required=True, help="Path to SHA256SUMS.txt from the release")
    parser.add_argument("--tag", required=True, help="Release tag, for example v0.1.0")
    return parser.parse_args()


def parse_checksums(path: Path) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in path.read_text().splitlines():
        parts = line.split(maxsplit=1)
        if len(parts) == 2:
            checksums[parts[1]] = parts[0]
    return checksums


def template_key(target: str, suffix: str) -> str:
    return f"{target.replace('-', '_')}_{suffix}"


def render_formula(tag: str, release_checksums: dict[str, str]) -> str:
    version = tag.removeprefix("v")
    assets = {target: f"sshoosh-{tag}-{target}.tar.gz" for target in TARGETS}
    values = {"version": version}

    for target, asset in assets.items():
        if asset not in release_checksums:
            raise SystemExit(f"missing checksum for {asset}")
        if not re.fullmatch(r"[0-9a-f]{64}", release_checksums[asset]):
            raise SystemExit(f"invalid checksum for {asset}")
        values[template_key(target, "asset")] = asset
        values[template_key(target, "sha")] = release_checksums[asset]

    repo_root = Path(__file__).resolve().parent.parent
    template = repo_root.joinpath("packaging/homebrew/sshoosh.rb.template").read_text()
    return template.format(**values)


def insert_after(lines: list[str], marker: str, additions: list[str], required_line: str) -> None:
    if required_line in lines:
        return
    try:
        index = lines.index(marker)
    except ValueError:
        return
    lines[index + 1:index + 1] = additions


def update_readme(path: Path) -> None:
    lines = path.read_text().splitlines()

    if FORMULA_ROW not in lines:
        formula_rows = [
            index
            for index, line in enumerate(lines)
            if line.startswith("| [") and "](Formula/" in line
        ]
        insert_at = formula_rows[-1] + 1 if formula_rows else lines.index("## How do I install these formulae?")
        lines.insert(insert_at, FORMULA_ROW)

    insert_after(lines, "`brew install puemos/tap/lareview`", ["", INSTALL_LINE], INSTALL_LINE)

    for index, line in enumerate(lines):
        if line.startswith("Or `brew tap puemos/tap`"):
            lines[index] = "Or `brew tap puemos/tap` and then `brew install lareview` or `brew install sshoosh`."
            break

    insert_after(lines, 'brew "lareview"', [BREWFILE_LINE], BREWFILE_LINE)
    path.write_text("\n".join(lines) + "\n")


def main() -> None:
    args = parse_args()
    tag = args.tag
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        raise SystemExit("--tag must look like vX.Y.Z")

    tap = Path(args.tap)
    checksums = parse_checksums(Path(args.checksums))

    formula_dir = tap / "Formula"
    formula_dir.mkdir(parents=True, exist_ok=True)
    formula_dir.joinpath("sshoosh.rb").write_text(render_formula(tag, checksums))
    update_readme(tap / "README.md")


if __name__ == "__main__":
    main()
