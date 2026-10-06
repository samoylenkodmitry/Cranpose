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
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DATA_BRANCH = 'perf-data'
SUMMARY_METRICS = ['fps', 'cpu_ms_per_frame', 'desired_to_present_p50_ms', 'janky_pct', 'pss_mb']


def git(*args, cwd):
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True,
                          text=True).stdout.strip()


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
        'subjects': [{'name': subject['name'], 'label': subject['label']} for subject in run['subjects']],
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
    if subprocess.run(['git', 'fetch', '-q', 'origin', DATA_BRANCH], cwd=REPO).returncode != 0:
        return []
    return json.loads(git('show', f'origin/{DATA_BRANCH}:index.json', cwd=REPO))['runs']


def data_tree(tree):
    """A worktree on the data branch, starting the branch when it is new."""
    fetched = subprocess.run(['git', 'fetch', '-q', 'origin', DATA_BRANCH], cwd=REPO).returncode == 0
    if not (tree / '.git').exists():
        git('worktree', 'add', '-q', '--force', '--detach', str(tree), 'HEAD', cwd=REPO)
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
    tree = data_tree(args.tree)
    stamp = run['started_at'][:10]
    device = run['device']['ro.product.model'].replace(' ', '-')
    file = f"runs/{stamp}-{run['kind']}-{(run.get('main') or 'manual')[:9]}-{device}.json"
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
    if not args.no_push:
        git('push', '-q', 'origin', f'HEAD:{DATA_BRANCH}', cwd=tree)
    print('published', file)


if __name__ == '__main__':
    main()
