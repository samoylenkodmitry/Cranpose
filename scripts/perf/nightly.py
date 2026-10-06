#!/usr/bin/env python3
"""Nightly comparison of the latest release against main on an attached
Android device.

1. Resolves main's commit and the newest release tag it contains.
2. Stops when the perf-data branch already holds that pair.
3. Builds the Cranpose benchmark app at the tag, installed as
   `dev.perfcompare.cranpose.release`, and at main. The tag builds in a
   worktree kept across nights, so its build caches stay warm.
4. Installs both under the device lock and runs `ab.py` over every scenario
   the release can draw.
5. Runs each scenario judged worse once more. A metric judged worse both
   times is a confirmed regression.

Writes `run.json`: ab.py's report with both commits and the confirmed
regressions, or the reason the night was skipped.
Usage: nightly.py --serial SERIAL --output DIR [--release-tree DIR]
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BENCH = Path('benchmarks/compose-vs-cranpose')
ANDROID = BENCH / 'cranpose-app/android'
SCENARIOS = ['gauntlet', 'feed', 'ticker', 'particles', 'layers', 'grid', 'grid_layer', 'deep',
             'deep_layer', 'workspace']

sys.path.insert(0, str(REPO / 'scripts'))
sys.path.insert(0, str(REPO / BENCH))
from android_robot_device import device_lock  # noqa: E402
from measure import Device  # noqa: E402
from publish import published_runs  # noqa: E402


def git(*args, cwd=REPO):
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True,
                          text=True).stdout.strip()


def measured_pairs():
    """The (main, release) pairs the data branch already holds."""
    return {(run['main'], run['release']) for run in published_runs() if run['kind'] == 'nightly'}


def release_has(tag, path, needle):
    return needle in git('show', f'{tag}:{path}')


def release_tree_at(tree, tag):
    if (tree / '.git').exists():
        git('checkout', '-q', '--detach', tag, cwd=tree)
    else:
        git('worktree', 'add', '-q', '--force', '--detach', str(tree), tag)
    return tree


def build(tree, suffix=None):
    command = ['./gradlew', '--no-daemon', '-q', ':app:assembleRelease']
    if suffix:
        command.append('-PperfCompareSuffix=' + suffix)
    # Each tree builds into its own target: one target shared by two trees
    # can hand one tree the other's library.
    environment = dict(os.environ, CARGO_TARGET_DIR=str(tree / BENCH / 'cranpose-app/target'))
    subprocess.run(command, cwd=tree / ANDROID, check=True, env=environment)
    return tree / ANDROID / 'app/build/outputs/apk/release/app-release.apk'


def compare(args, scenarios, output, labels):
    subprocess.run([sys.executable, str(REPO / BENCH / 'ab.py'), '--serial', args.serial,
                    '--a', 'cranpose-release', '--b', 'cranpose', '--label-a', labels[0],
                    '--label-b', labels[1], '--scenarios', ','.join(scenarios),
                    '--output', str(output)], check=True)
    return json.loads((output / 'ab.json').read_text())


def confirmed(first, second):
    """Per scenario, the metrics both runs judged worse."""
    earlier = {scenario['scenario']: scenario['verdicts'] for scenario in first['scenarios']}
    found = {}
    for scenario in second['scenarios']:
        metrics = sorted(metric for metric, verdict in scenario['verdicts'].items()
                         if verdict == 'worse' and earlier[scenario['scenario']][metric] == 'worse')
        if metrics:
            found[scenario['scenario']] = metrics
    return found


def nightly(args):
    main = git('rev-parse', 'HEAD')
    release = git('describe', '--tags', '--abbrev=0', '--match', 'v*')
    if (main, release) in measured_pairs():
        return {'skipped': f'main {main[:9]} was already measured against {release}'}
    if not release_has(release, ANDROID / 'app/build.gradle.kts', 'perfCompareSuffix'):
        return {'skipped': f'{release} predates installing a second benchmark build'}
    # A scenario the release's app does not know falls back to the feed, so
    # only the ones its measure table lists are compared.
    scenarios = [scenario for scenario in SCENARIOS
                 if release_has(release, BENCH / 'measure.py', f"'{scenario}': '")]
    release_apk = build(release_tree_at(args.release_tree, release), '.release')
    main_apk = build(REPO)
    with device_lock(args.serial):
        device = Device(args.serial)
        device.install('cranpose-release', release_apk)
        device.install('cranpose', main_apk)
    labels = (release, main[:9])
    report = compare(args, scenarios, args.output / 'ab', labels)
    worse = [scenario['scenario'] for scenario in report['scenarios']
             if 'worse' in scenario['verdicts'].values()]
    regressions = confirmed(report, compare(args, worse, args.output / 'recheck', labels)) if worse else {}
    return {**report, 'kind': 'nightly', 'main': main, 'release': release,
            'release_commit': git('rev-list', '-n', '1', release), 'confirmed_regressions': regressions}


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--release-tree', type=Path, default=Path.home() / 'perf-nightly' / 'release-tree',
                        help='worktree the release builds in, kept across nights')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    args.release_tree.parent.mkdir(parents=True, exist_ok=True)
    run = nightly(args)
    (args.output / 'run.json').write_text(json.dumps(run, indent=1))
    print(run.get('skipped') or f"compared {run['release']} against main {run['main'][:9]}: "
          f"{len(run['scenarios'])} scenarios, regressions {run['confirmed_regressions'] or 'none'}")


if __name__ == '__main__':
    main()
