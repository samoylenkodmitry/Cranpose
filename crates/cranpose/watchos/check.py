#!/usr/bin/env python3
"""Build a shared composable for both watchOS targets and launch the simulator."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

from smoke import smoke


def main():
    templates = Path(__file__).resolve().parent
    workspace = templates.parents[2]
    destination = workspace / 'target/watchos'
    fixture = destination / 'fixture'
    fixture.mkdir(parents=True, exist_ok=True)
    version = tomllib.loads((workspace / 'Cargo.toml').read_text())['workspace']['package']['version']
    (fixture / 'Cargo.toml').write_text(f'''[package]
name = "cranpose-watch-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
path = "lib.rs"
[dependencies]
cranpose = {{ version = {json.dumps(version)} }}
[package.metadata.cranpose.watchos]
entry = "WatchApp"
bundle-id = "org.cranpose.watch.fixture"
name = "Cranpose"
''')
    shutil.copyfile(templates / 'tests/fixture.rs', fixture / 'lib.rs')
    for target in ('simulator', 'device'):
        subprocess.run([sys.executable, str(templates / 'build.py'),
                        '--manifest', str(fixture / 'Cargo.toml'), '--framework-source', str(workspace),
                        '--target', target, '--output', str(destination / target)], check=True)
        shutil.make_archive(str(destination / target), 'zip', destination / target, 'CranposeWatch.app')
    smoke(destination / 'simulator/CranposeWatch.app', destination / 'evidence')


if __name__ == '__main__':
    main()
