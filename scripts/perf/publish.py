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
import json
import os
import shutil
import subprocess
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DATA_BRANCH = 'perf-data'
SUMMARY_METRICS = ['fps', 'first_frame_s', 'cpu_ms_per_frame', 'desired_to_present_p50_ms', 'janky_pct',
                   'ram_mb', 'gpu_ram_mb', 'cpu_mhz', 'gpu_mhz']


# Attempts at a push GitHub refuses, and the seconds between them, times the
# attempt: on 2026-10-07 a push failed on an Internal Server Error.
PUSH_ATTEMPTS = 3
PUSH_DELAY_S = 2


# Answers git's request for credentials with the job's token.
TOKEN_HELPER = '!f() { test "$1" = get && echo username=x-access-token && echo "password=$GH_TOKEN"; }; f'


def credentials():
    """The environment git runs in. A job that holds a token (`GH_TOKEN`) gives
    it to git when the server asks for credentials, in place of the
    machine's keychain helper: a Mac's login keychain is locked while nobody
    is at the machine, and the helper then fails the push with "failed to get:
    -25308" (the nightly of 2026-10-08). A header would reach the server even
    when `actions/checkout`'s own reaches it, which GitHub refuses as a
    duplicate."""
    if not os.environ.get('GH_TOKEN'):
        return None
    return {**os.environ, 'GIT_CONFIG_COUNT': '2',
            'GIT_CONFIG_KEY_0': 'credential.helper', 'GIT_CONFIG_VALUE_0': '',
            'GIT_CONFIG_KEY_1': 'credential.helper', 'GIT_CONFIG_VALUE_1': TOKEN_HELPER}


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
        # A run measured from each launch says so; an older one measured a
        # window after a warm-up.
        'protocol': run.get('protocol', {}),
        'scenarios': {
            scenario['scenario']: {
                'legs': len(scenario['legs']),
                # Why a subject has no legs: its first failure.
                'failures': {failure['subject']: failure['error']
                             for failure in reversed(scenario.get('failures', []))},
                'summary': {name: {metric: scenario['summary'][name][metric]
                                   for metric in SUMMARY_METRICS if metric in scenario['summary'][name]}
                            for name in names},
                'verdicts': scenario['verdicts'],
            }
            for scenario in run['scenarios']
        },
        'confirmed_regressions': run.get('confirmed_regressions', {}),
    }


def fetch_data_branch(cwd):
    return subprocess.run(['git', 'fetch', '-q', 'origin', DATA_BRANCH], cwd=cwd, env=credentials(),
                          capture_output=True, text=True)


def published_runs():
    """The runs the data branch's index lists, or none before its first run."""
    if fetch_data_branch(REPO).returncode != 0:
        return []
    return json.loads(git('show', f'origin/{DATA_BRANCH}:index.json', cwd=REPO))['runs']


def worktree(tree, revision):
    """`tree` as a worktree of this checkout, made at `revision` when it is
    not one. A tree whose record the checkout lost, because `actions/checkout`
    made the checkout's .git again (macm3, 2026-10-08), is made again: git
    says "not a git repository" in it."""
    if (tree / '.git').exists() and subprocess.run(
            ['git', 'rev-parse', '--git-dir'], cwd=tree, capture_output=True).returncode == 0:
        return
    shutil.rmtree(tree, ignore_errors=True)
    git('worktree', 'prune', cwd=REPO)
    git('worktree', 'add', '-q', '--force', '--detach', str(tree), revision, cwd=REPO)


def data_tree(tree):
    """A worktree on the data branch's latest commit, starting the branch
    when it is new. Fetched in the tree: it may be a worktree of another
    clone than this script's."""
    worktree(tree, 'HEAD')
    fetched = fetch_data_branch(tree)
    if fetched.returncode == 0:
        # Detached at the data branch's head: the branch may be checked out in
        # another worktree, and the push names it.
        # Forced and cleaned: a run whose commit failed left its files in the
        # tree, and the branch has moved since.
        git('checkout', '-q', '-f', '--detach', f'origin/{DATA_BRANCH}', cwd=tree)
        git('clean', '-q', '-f', '-d', cwd=tree)
    elif "couldn't find remote ref" in fetched.stderr:
        git('checkout', '-q', '--orphan', DATA_BRANCH, cwd=tree)
        git('rm', '-q', '-r', '-f', '.', cwd=tree)
        (tree / 'index.json').write_text(json.dumps({'runs': []}, indent=1) + '\n')
    else:
        # A branch that exists must not be started again over a fetch that failed.
        raise RuntimeError(f'git fetch origin {DATA_BRANCH} failed: {fetched.stderr.strip()}')
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
    # Compact: every leg keeps the time of each of its frames since the launch.
    (tree / file).write_text(json.dumps(run, separators=(',', ':')) + '\n')
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
    # Unsigned: a Mac that signs the user's commits has no one to answer its
    # pinentry at night (macm3, 2026-10-08), and the data holds no user's work.
    git('-c', 'commit.gpgsign=false', 'commit', '-q', '-m', f"{run['kind']} {stamp}: {what} on {device}", cwd=tree)


if __name__ == '__main__':
    main()
