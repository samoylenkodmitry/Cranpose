#!/usr/bin/env python3
"""Point `apps/isolated-demo` at a published cranpose version.

The isolated demo deliberately consumes cranpose from crates.io (it is the
canary that proves a release is usable by a real downstream crate), so it can
only be moved onto a release *after* that release is on the registry —
`publish.yml` defers the bump for exactly that reason. Left undone, the next
`scripts/check_cranpose_versions.py` run fails and main goes red until someone
bumps it by hand; `publish.yml` now runs this automatically once the crates are
live.

Usage:
    scripts/sync_isolated_demo.py            # sync to the workspace version
    scripts/sync_isolated_demo.py 0.1.73     # sync to an explicit version
    scripts/sync_isolated_demo.py v0.1.73    # a release tag works too

Only rewrites the manifest; run `cargo update --manifest-path
apps/isolated-demo/Cargo.toml -p cranpose -p cranpose-core` afterwards to move
the demo's lockfile (that step needs the version to be resolvable).
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "apps/isolated-demo/Cargo.toml"

# `cranpose-foo = { version = "x", ... }` (inline table) and `cranpose-foo = "x"`
# (bare string), in [dependencies] and any [target.'cfg(..)'.dependencies].
INLINE_TABLE = re.compile(
    r'(?P<head>^[ \t]*cranpose[\w-]*[ \t]*=[ \t]*\{[^}\n]*?version[ \t]*=[ \t]*")'
    r'(?P<version>[^"]+)'
    r'(?P<tail>")',
    re.MULTILINE,
)
BARE_STRING = re.compile(
    r'(?P<head>^[ \t]*cranpose[\w-]*[ \t]*=[ \t]*")(?P<version>[^"]+)(?P<tail>")',
    re.MULTILINE,
)


def workspace_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as handle:
        return tomllib.load(handle)["workspace"]["package"]["version"]


def main() -> int:
    version = sys.argv[1] if len(sys.argv) > 1 else workspace_version()
    version = version.lstrip("v")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.\-]+)?", version):
        print(f"Not a semver version: {version!r}", file=sys.stderr)
        return 1

    text = MANIFEST.read_text()
    changed: list[str] = []

    def rewrite(match: re.Match[str]) -> str:
        name = match.group(0).split("=", 1)[0].strip()
        if match.group("version") != version:
            changed.append(f"{name} {match.group('version')} -> {version}")
        return f"{match.group('head')}{version}{match.group('tail')}"

    updated = BARE_STRING.sub(rewrite, INLINE_TABLE.sub(rewrite, text))

    if not changed:
        print(f"apps/isolated-demo already depends on cranpose {version}.")
        return 0

    MANIFEST.write_text(updated)
    for line in changed:
        print(f"apps/isolated-demo: {line}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
