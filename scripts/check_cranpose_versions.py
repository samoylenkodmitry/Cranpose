#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]


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


def root_lock_versions(path: Path) -> dict[str, set[str]]:
    versions: dict[str, set[str]] = {}
    package_name: str | None = None

    for line in path.read_text().splitlines():
        stripped = line.strip()
        if stripped == "[[package]]":
            package_name = None
            continue
        if stripped.startswith("name = "):
            package_name = stripped.split("=", 1)[1].strip().strip('"')
            continue
        if package_name is None or not package_name.startswith("cranpose"):
            continue
        if stripped.startswith("version = "):
            version = stripped.split("=", 1)[1].strip().strip('"')
            versions.setdefault(package_name, set()).add(version)
            package_name = None

    return versions


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

    lock_versions = root_lock_versions(ROOT / "Cargo.lock")
    for name in sorted(expected_package_names.difference(lock_versions)):
        failures.append(f"Cargo.lock is missing workspace package {name}")

    for name, versions in sorted(lock_versions.items()):
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

    settings = (ROOT / "apps/isolated-demo/android/settings.gradle.kts").read_text()
    plugin = re.search(
        r'id\("dev\.cranpose\.android"\)\s+version\s+"([^"]+)"', settings
    )
    if plugin is None:
        failures.append(
            "apps/isolated-demo/android/settings.gradle.kts does not pin the "
            "dev.cranpose.android plugin version"
        )
    elif plugin.group(1) != workspace_version:
        failures.append(
            "apps/isolated-demo Gradle plugin dev.cranpose.android is "
            f"{plugin.group(1)}, expected {workspace_version}"
        )

    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1

    print(f"cranpose package versions are aligned at {workspace_version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
