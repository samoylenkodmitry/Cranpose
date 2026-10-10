#!/usr/bin/env python3
"""Opens an issue for a night's confirmed regressions.

Names the commits main gained since the previous night measured it, gives
each regressed scenario's medians on both builds, and the command that
reproduces the comparison. Nights without a confirmed regression report
nothing.
Usage: report.py --run RUN_JSON (with GH_TOKEN set)
"""

import argparse
import json
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DATA_BRANCH = 'perf-data'
LABEL = 'perf-regression'
UNITS = {'fps': 'fps', 'cpu_ms_per_frame': 'ms CPU/frame', 'desired_to_present_p50_ms': 'ms to screen'}
STARTS = {'first': 'first start after install', 'second': 'second start'}


def previous_main(run):
    """The main commit of the night before, from the data branch."""
    shown = subprocess.run(['git', 'show', f'origin/{DATA_BRANCH}:index.json'], cwd=REPO,
                           capture_output=True, text=True)
    if shown.returncode != 0:
        return None
    nights = [entry for entry in json.loads(shown.stdout)['runs']
              if entry['kind'] == 'nightly' and entry['main'] != run['main']
              and entry['started_at'] < run['started_at']]
    return nights[-1]['main'] if nights else None


def body(run):
    release, main = run['release'], run['main']
    names = [subject['name'] for subject in run['subjects']]
    since = previous_main(run)
    commits = (f'Main gained these commits since the last night measured it: '
               f'[{since[:9]}...{main[:9]}](https://github.com/samoylenkodmitry/Cranpose/compare/{since}...{main}).'
               if since else f'This is the first night measured; main is {main[:9]}.')
    lines = [
        f'The nightly comparison on the {run["device"]["ro.product.model"]} found main ({main[:9]}) '
        f'slower than {release}, twice in a row, in the scenarios below. {commits}',
        '',
        '| scenario | start | metric | ' + f'{release} | main ({main[:9]}) | change |',
        '|---|---|---|---|---|---|',
    ]
    for scenario in run['scenarios']:
        for start, metrics in run['confirmed_regressions'].get(scenario['scenario'], {}).items():
            summary = scenario['starts'][start]['summary']
            for metric in metrics:
                before, after = summary[names[0]][metric], summary[names[1]][metric]
                lines.append(f'| {scenario["scenario"]} | {STARTS[start]} | {UNITS[metric]} | {before:.2f} | '
                             f'{after:.2f} | {(after - before) / before:+.1%} |')
    lines += [
        '',
        'Each value is the median of the first run\'s legs of that start (each one launch measured from '
        'its start for 10 s, its first seconds included; in each pair, A then B, or B then A, each starts '
        'first with its data cleared, as its install leaves it, then a second time); '
        'a second run judged the same metrics worse. Reproduce on the device with:',
        '',
        '```bash',
        'python3 benchmarks/compose-vs-cranpose/ab.py --serial SERIAL --a cranpose-release --b cranpose '
        f'--scenarios {",".join(run["confirmed_regressions"])} --output OUTPUT',
        '```',
        '',
        f'The full run is on the `{DATA_BRANCH}` branch.',
    ]
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--run', required=True, type=Path)
    args = parser.parse_args()
    run = json.loads(args.run.read_text())
    if run.get('skipped') or not run.get('confirmed_regressions'):
        print('no confirmed regression')
        return
    subprocess.run(['gh', 'label', 'create', LABEL, '--force', '--color', 'B60205',
                    '--description', 'Confirmed by the nightly release-against-main comparison'],
                   cwd=REPO, check=True)
    title = (f'Nightly: main {run["main"][:9]} slower than {run["release"]} on '
             f'{", ".join(run["confirmed_regressions"])}')
    subprocess.run(['gh', 'issue', 'create', '--title', title, '--label', LABEL, '--body', body(run)],
                   cwd=REPO, check=True)


if __name__ == '__main__':
    main()
