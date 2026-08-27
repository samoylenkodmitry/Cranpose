#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]

# What a Cargo.lock records for a crate resolved from crates.io. A lockfile
# entry without it was resolved from somewhere else -- a `[patch]`, a path
# dependency, a git dependency.
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"


def load_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def dependency_version(spec: object) -> str | None:
    if isinstance(spec, str):
        return spec
    if isinstance(spec, dict):
        version = spec.get("version")
        return version if isinstance(version, str) else None
    return None


def cranpose_lock_packages(path: Path) -> list[dict]:
    packages = load_toml(path).get("package")
    if not isinstance(packages, list):
        return []
    return [
        package
        for package in packages
        if isinstance(package, dict)
        and isinstance(package.get("name"), str)
        and package["name"].startswith("cranpose")
    ]


def lock_versions(packages: list[dict]) -> dict[str, set[str]]:
    versions: dict[str, set[str]] = {}
    for package in packages:
        version = package.get("version")
        if isinstance(version, str):
            versions.setdefault(package["name"], set()).add(version)
    return versions


def check_published_lock(
    path: Path, workspace_version: str, failures: list[str]
) -> None:
    """Assert a lockfile resolves every Cranpose crate from crates.io.

    `apps/isolated-demo` is the canary that proves a release is consumable by
    an outside project, so its lockfile has to pin the *published* crates. A
    local `[patch]` -- the one `cargo xtask binary-size
    --patch-workspace-cranpose` applies -- makes cargo drop the `source` and
    `checksum` lines, which silently turns the canary into a path build that
    verifies nothing.
    """
    relative = path.relative_to(ROOT)
    packages = cranpose_lock_packages(path)
    if not packages:
        failures.append(f"{relative} locks no cranpose packages")
        return

    for package in packages:
        name = package["name"]
        source = package.get("source")
        if source != CRATES_IO_SOURCE:
            origin = source if isinstance(source, str) else "a local path"
            failures.append(
                f"{relative} resolves {name} from {origin}, expected the "
                f"published crate at {CRATES_IO_SOURCE}"
            )
        elif not isinstance(package.get("checksum"), str):
            failures.append(f"{relative} package {name} has no checksum")
        version = package.get("version")
        if version != workspace_version:
            failures.append(
                f"{relative} package {name} is {version}, "
                f"expected {workspace_version}"
            )


def dependency_tables(manifest: dict) -> list[dict]:
    tables: list[dict] = []
    dependencies = manifest.get("dependencies")
    if isinstance(dependencies, dict):
        tables.append(dependencies)

    targets = manifest.get("target")
    if isinstance(targets, dict):
        for target in targets.values():
            if isinstance(target, dict):
                target_dependencies = target.get("dependencies")
                if isinstance(target_dependencies, dict):
                    tables.append(target_dependencies)

    return tables


def main() -> int:
    failures: list[str] = []

    root_manifest = load_toml(ROOT / "Cargo.toml")
    workspace = root_manifest["workspace"]
    workspace_version = workspace["package"]["version"]

    expected_package_names: set[str] = set()
    workspace_dependencies = workspace.get("dependencies", {})
    for name, spec in sorted(workspace_dependencies.items()):
        if not name.startswith("cranpose"):
            continue
        expected_package_names.add(name)
        version = dependency_version(spec)
        if version != workspace_version:
            failures.append(
                f"workspace dependency {name} is {version}, expected {workspace_version}"
            )

    root_versions = lock_versions(cranpose_lock_packages(ROOT / "Cargo.lock"))
    for name in sorted(expected_package_names.difference(root_versions)):
        failures.append(f"Cargo.lock is missing workspace package {name}")

    for name, versions in sorted(root_versions.items()):
        if versions != {workspace_version}:
            found = ", ".join(sorted(versions))
            failures.append(
                f"Cargo.lock package {name} has {found}, expected {workspace_version}"
            )

    isolated_manifest = load_toml(ROOT / "apps/isolated-demo/Cargo.toml")
    for table in dependency_tables(isolated_manifest):
        for name, spec in sorted(table.items()):
            if not name.startswith("cranpose"):
                continue
            version = dependency_version(spec)
            if version != workspace_version:
                failures.append(
                    "apps/isolated-demo dependency "
                    f"{name} is {version}, expected {workspace_version}"
                )

    check_published_lock(
        ROOT / "apps/isolated-demo/Cargo.lock", workspace_version, failures
    )

    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1

    print(f"cranpose package versions are aligned at {workspace_version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
