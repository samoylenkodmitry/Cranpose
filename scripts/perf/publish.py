#!/usr/bin/env python3
"""Adds a comparison run to the perf-data branch the dashboard reads.

The run goes under `runs/`. `index.json` gains one entry per run, holding
each scenario's medians and verdicts, so the dashboard draws its trends from
the index and opens a run's file only for its legs. A run that was skipped
publishes nothing.
Usage: publish.py --run RUN_JSON [--tree DIR] [--no-push]
       publish.py --published KIND DEVICE COMMIT   exits 0 when such a run is in
                                                  the index, 1 when not
"""

import argparse
import base64
import json
import os
import subprocess
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DATA_BRANCH = 'perf-data'
SUMMARY_METRICS = ['fps', 'cpu_ms_per_frame', 'desired_to_present_p50_ms', 'janky_pct', 'ram_mb',
                   'gpu_ram_mb', 'cpu_mhz', 'gpu_mhz']


# Attempts at a push GitHub refuses, and the seconds between them, times the
# attempt: on 2026-10-07 a push failed on an Internal Server Error.
PUSH_ATTEMPTS = 3
PUSH_DELAY_S = 2


def credentials():
    """The environment git runs in. A job that holds a token (`GH_TOKEN`) makes
    its pushes with it, as `actions/checkout` fetches, and leaves the
    keychain helper out: a Mac's login keychain is locked while nobody is
    at the machine, and the helper then fails the push with "failed to get:
    -25308" (the nightly of 2026-10-08)."""
    token = os.environ.get('GH_TOKEN')
    if not token:
        return None
    header = base64.b64encode(f'x-access-token:{token}'.encode()).decode()
    return {**os.environ, 'GIT_CONFIG_COUNT': '2',
            'GIT_CONFIG_KEY_0': 'credential.helper', 'GIT_CONFIG_VALUE_0': '',
            'GIT_CONFIG_KEY_1': 'http.https://github.com/.extraheader',
            'GIT_CONFIG_VALUE_1': f'AUTHORIZATION: basic {header}'}


def git(*args, cwd):
    result = subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True, env=credentials())
    if result.returncode != 0:
        raise RuntimeError(f'git {" ".join(args)} failed: {result.stderr.strip()}')
    return result.stdout.strip()


def index_entry(run, file):
    """What the dashboard needs of a run without opening it."""
    names = [subject['name'] for subject in run['subjects']]
    return {
        'file': file,
        'kind': run['kind'],
        'started_at': run['started_at'],
        'device': run['device']['ro.product.model'],
        'main': run.get('main'),
        'release': run.get('release'),
        'subjects': [{key: subject[key] for key in ('name', 'label', 'source') if key in subject}
                     for subject in run['subjects']],
        'duration_s': run.get('duration_s'),
        'scenarios': {
            scenario['scenario']: {
                'legs': len(scenario['legs']),
                'summary': {name: {metric: scenario['summary'][name][metric]
                                   for metric in SUMMARY_METRICS if metric in scenario['summary'][name]}
                            for name in names},
                'verdicts': scenario['verdicts'],
            }
            for scenario in run['scenarios']
        },
        'confirmed_regressions': run.get('confirmed_regressions', {}),
    }


def published_runs():
    """The runs the data branch's index lists, or none before its first run."""
    if subprocess.run(['git', 'fetch', '-q', 'origin', DATA_BRANCH], cwd=REPO, env=credentials()).returncode != 0:
        return []
    return json.loads(git('show', f'origin/{DATA_BRANCH}:index.json', cwd=REPO))['runs']


def data_tree(tree):
    """A worktree on the data branch's latest commit, starting the branch
    when it is new. Fetched in the tree: it may be a worktree of another
    clone than this script's."""
    if not (tree / '.git').exists():
        git('worktree', 'add', '-q', '--force', '--detach', str(tree), 'HEAD', cwd=REPO)
    fetched = subprocess.run(['git', 'fetch', '-q', 'origin', DATA_BRANCH], cwd=tree, env=credentials()).returncode == 0
    if fetched:
        # Detached at the data branch's head: the branch may be checked out in
        # another worktree, and the push names it.
        git('checkout', '-q', '--detach', f'origin/{DATA_BRANCH}', cwd=tree)
    else:
        git('checkout', '-q', '--orphan', DATA_BRANCH, cwd=tree)
        git('rm', '-q', '-r', '-f', '.', cwd=tree)
        (tree / 'index.json').write_text(json.dumps({'runs': []}, indent=1) + '\n')
    return tree


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--run', type=Path)
    parser.add_argument('--published', nargs=3, metavar=('KIND', 'DEVICE', 'COMMIT'))
    parser.add_argument('--tree', type=Path, default=Path.home() / 'perf-nightly' / 'data-tree')
    parser.add_argument('--no-push', action='store_true')
    args = parser.parse_args()
    if args.published:
        kind, device, commit = args.published
        found = any(run['kind'] == kind and run['device'] == device and run.get('main') == commit
                    for run in published_runs())
        print(f'{kind} on {device} at {commit[:9]}:', 'published' if found else 'not published')
        raise SystemExit(0 if found else 1)
    if not args.run:
        parser.error('--run or --published is required')
    run = json.loads(args.run.read_text())
    if run.get('skipped'):
        print('nothing to publish:', run['skipped'])
        return
    stamp = run['started_at'][:10]
    device = run['device']['ro.product.model'].replace(' ', '-')
    file = f"runs/{stamp}-{run['kind']}-{(run.get('main') or 'manual')[:9]}-{device}.json"
    # A refused push commits again on the branch's latest commit and tries
    # once more.
    for attempt in range(1, PUSH_ATTEMPTS + 1):
        tree = data_tree(args.tree)
        commit_run(tree, run, file, stamp, device)
        if args.no_push:
            break
        try:
            git('push', '-q', 'origin', f'HEAD:{DATA_BRANCH}', cwd=tree)
            break
        except RuntimeError as refused:
            if attempt == PUSH_ATTEMPTS:
                raise
            print(f'{refused}; trying again', flush=True)
            time.sleep(PUSH_DELAY_S * attempt)
    print('published', file)


def commit_run(tree, run, file, stamp, device):
    """Adds `run` as `file` and to the index, and commits both."""
    (tree / file).parent.mkdir(parents=True, exist_ok=True)
    (tree / file).write_text(json.dumps(run, indent=1) + '\n')
    index = json.loads((tree / 'index.json').read_text())
    index['runs'] = [entry for entry in index['runs'] if entry['file'] != file]
    index['runs'].append(index_entry(run, file))
    index['runs'].sort(key=lambda entry: entry['started_at'])
    (tree / 'index.json').write_text(json.dumps(index, indent=1) + '\n')
    git('add', 'index.json', file, cwd=tree)
    if run.get('release'):
        what = f"{run['release']} against main {(run.get('main') or '')[:9]}"
    else:
        what = ', '.join(subject['name'] for subject in run['subjects'])
    git('commit', '-q', '-m', f"{run['kind']} {stamp}: {what} on {device}", cwd=tree)


if __name__ == '__main__':
    main()
