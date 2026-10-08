#!/usr/bin/env python3
"""Opens, or updates, the pull request that moves the gauntlet's frameworks
to the releases the night built and measured.

Reads `versions.py bump`'s record: the pins that moved and still built, with
the rest of their app folders, are committed on `perf/framework-versions` and
proposed against main; a pin put back because its build failed is listed as
not moved.
Usage: propose_versions.py --record FILE
"""

import argparse
import json
import os
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BRANCH = 'perf/framework-versions'
BENCHMARK = 'benchmarks/compose-vs-cranpose'
TITLE = "Move the gauntlet's frameworks to their latest releases"


def run(*command):
    return subprocess.run(command, cwd=REPO, check=True, capture_output=True, text=True).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--record', required=True, type=Path)
    args = parser.parse_args()
    record = json.loads(args.record.read_text()) if args.record.exists() else {}
    moved = {pin: change for pin, change in record.items() if not change['reverted']}
    kept = {pin: change for pin, change in record.items() if change['reverted']}
    if not moved:
        print('no pin moved' + (f"; {', '.join(kept)} did not build at the latest release" if kept else ''))
        return
    lines = [f"- {pin}: {change['from']} → {change['to']}" for pin, change in moved.items()]
    lines += [f"- {pin}: {change['to']} did not build, so it stays at {change['from']}"
              for pin, change in kept.items()]
    run_url = f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/{os.environ.get('GITHUB_REPOSITORY', '')}" \
              f"/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}"
    body = '\n'.join([
        'The nightly built and measured these frameworks at their latest stable releases:',
        '',
        *lines,
        '',
        f'Night: {run_url}',
        '',
        'A pull request the workflow opens starts no checks of its own: push to it, or re-run them, before merging.',
    ])
    run('git', 'config', 'user.name', 'github-actions[bot]')
    run('git', 'config', 'user.email', '41898282+github-actions[bot]@users.noreply.github.com')
    run('git', 'checkout', '-q', '-B', BRANCH)
    # The moved pins' own app folders, their lock files included: what the
    # night's builds rewrote elsewhere is not part of the proposal.
    folders = sorted({f'{BENCHMARK}/{Path(file).parts[0]}' for change in moved.values() for file in change['files']})
    run('git', 'add', '-u', '--', *folders)
    # Unsigned: the author is the bot, and a Mac that signs the user's commits
    # has no one to answer its pinentry at night.
    run('git', '-c', 'commit.gpgsign=false', 'commit', '-q', '-m', TITLE, '-m', '\n'.join(lines))
    run('git', 'push', '-q', '-f', 'origin', BRANCH)
    existing = run('gh', 'pr', 'list', '--head', BRANCH, '--state', 'open', '--json', 'number', '--jq', '.[0].number')
    if existing:
        run('gh', 'pr', 'edit', existing, '--body', body)
        print(f'updated pull request #{existing}')
    else:
        print(run('gh', 'pr', 'create', '--base', 'main', '--head', BRANCH, '--title', TITLE, '--body', body))
    run('git', 'checkout', '-q', '--detach', 'HEAD~1')


if __name__ == '__main__':
    main()
