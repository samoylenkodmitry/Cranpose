#!/usr/bin/env python3
"""Measure the shared APKs on independent Android devices and publish each completed run."""
import argparse
import subprocess
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path


def measure(directory, command):
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / 'capture.log').open('w') as log:
        subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    return directory / 'frameworks.json'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--serial', required=True)
    args = parser.parse_args()
    apks = args.output / 'apks'
    mate = args.output / 'frameworks'
    stick = args.output / 'mi-tv'
    commands = {mate: ['just', 'perf-measure-frameworks', mate, apks, args.serial],
                stick: ['just', 'perf-mi-tv', stick, apks]}
    failed = []
    with ThreadPoolExecutor(max_workers=len(commands)) as pool:
        pending = {pool.submit(measure, folder, command): folder for folder, command in commands.items()}
        for future in as_completed(pending):
            folder = pending[future]
            try:
                report = future.result()
                subprocess.run(['just', 'perf-publish', report], check=True)
            except (OSError, subprocess.CalledProcessError) as error:
                print(f'{folder.name}: {error}', flush=True)
                failed.append(folder.name)
                log = folder / 'capture.log'
                if log.exists():
                    print(log.read_text(errors='replace')[-4000:], flush=True)
    if failed:
        raise SystemExit('Failed Android runs: ' + ', '.join(failed))


if __name__ == '__main__':
    main()
