#!/usr/bin/env python3
"""Measures every framework's app on one scenario, for the dashboard's
framework comparison.

Every app is measured in rounds, every app once a round, the order reversed
each round so heat and time fall on all alike. Legs are `ab.py`'s: one launch
measured from its start command for `--run` seconds, the same span for every
app, with nothing left out. The first round is each app's first launch after
its install. Writes `frameworks.json` in the dashboard's run format, kind
`frameworks`: device, subjects, every leg with the time of each of its frames
since the launch, and per-app medians.
`--install DIR` first installs each app's `APP.apk` from DIR, as the
nightly hands over the builds macm3 made. `--browser DIR` measures the pages
`build_apps.sh browser DIR` built instead, each open in Chrome on the phone at
`--tier`, and writes a run of kind `browser`.
Usage: frameworks.py --serial SERIAL --output DIR [--apps compose,cranpose,...]
                     [--install DIR | --browser DIR --tier N] [--main COMMIT]
"""

import argparse
import json
import re
import time
from datetime import datetime, timezone
from pathlib import Path

from ab import DECIDING, REPORTED, leg_record, summarize
from browser import BrowserServer
from measure import (APPS, CHROME, HEAVY, HERE, REMOTE_WINDOW, AppTarget, Device, PageTarget, device_lock,
                     measure_run)
import versions

DEFAULT_APPS = 'compose,cranpose,views,flutter,rn,nativescript,lynx,maui,avalonia,egui,slint,web'


def measure_frameworks(args):
    """Runs the whole comparison: the caller holds the device lock."""
    args.load, args.screenshots = 'heavy', False
    args.started = time.monotonic()
    args.output.mkdir(parents=True, exist_ok=True)
    device = Device(args.serial)
    apps = args.apps.split(',')
    if args.browser:
        server = BrowserServer(args.browser)
        device.adb('reverse', f'tcp:{server.port}', f'tcp:{server.port}')
        targets = {app: PageTarget(server, app, args.tier) for app in apps}
    else:
        targets = {app: AppTarget(app) for app in apps}
        for app in apps:
            if args.install and (apk := args.install / f'{app}.apk').exists():
                device.install(app, apk)
    device.adb('push', str(HERE / 'perf_window.sh'), REMOTE_WINDOW)
    # The versions macm3 built with, and the browser or WebView the web page
    # runs in.
    folder = args.browser or args.install
    built = json.loads((folder / 'versions.json').read_text()) if folder and (
        folder / 'versions.json').exists() else {}
    chrome = re.search(r'versionName=(\S+)', device.shell('dumpsys', 'package', CHROME))
    if args.browser:
        built['web'] = f'Chrome {chrome.group(1)}' if chrome else 'Chrome'
    else:
        webview = device.shell('dumpsys', 'package', 'com.google.android.webview')
        if match := re.search(r'versionName=(\S+)', webview):
            built['web'] = f'WebView {match.group(1)}, {built.get("web") or versions.version("web", "android")}'
    # An app whose first launch fails leaves its legs out and the run goes
    # on: a page can fail where the browser lacks what its framework needs.
    legs, failures, running = [], [], list(apps)
    for round_index in range(args.rounds):
        for app in list(running if round_index % 2 == 0 else running[::-1]):
            try:
                run = measure_run(device, targets[app], args.scenario, args, args.output)
            except (RuntimeError, ValueError) as failure:
                print(f'{app:10} failed: {failure}', flush=True)
                # Kept, so the dashboard shows why the app has no frames.
                failures.append({'subject': app, 'round': round_index, 'error': str(failure)[:300]})
                if round_index == 0:
                    running.remove(app)
                continue
            legs.append(leg_record(run, app, len(legs)))
            print(f'{app:10} fps {run["fps"]:5.1f} cpu/f {run["cpu_ms_per_frame"]:6.1f} '
                  f'frames {run["frames"]:4d} first frame {run["first_frame_s"]:4.1f} s '
                  f'run {run["run_s"]:4.1f} s', flush=True)
    summary = summarize(legs, apps, [*DECIDING, *REPORTED])
    identity = {key: device.shell('getprop', key).strip() for key in
                ['ro.product.model', 'ro.build.version.release', 'ro.hardware']}
    if args.browser:
        # The dashboard lists a device's latest comparison: the browser's has
        # a card of its own.
        identity['ro.product.model'] += f' · Chrome {chrome.group(1).split(".")[0]}' if chrome else ' · Chrome'
        subjects = [versions.subject(app, 'browser', built, args.release) for app in apps]
    else:
        subjects = [{**versions.subject(app, 'android', built, args.release), 'package': APPS[app]['package']}
                    for app in apps]
    return {
        'kind': 'browser' if args.browser else 'frameworks',
        'started_at': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'main': args.main,
        'release': args.release,
        'device': identity,
        'subjects': subjects,
        'protocol': {'run_s': args.run, 'from_launch': True, 'rounds': args.rounds},
        'scenarios': [{
            'scenario': args.scenario,
            'extras': f'tier {args.tier}, in Chrome' if args.browser else (
                HEAVY.get(args.scenario, '') + ' ' + args.extra).strip(),
            'legs': legs,
            'failures': failures,
            'summary': summary,
            'verdicts': {},
        }],
        'duration_s': round(time.monotonic() - args.started),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apps', help='by default every app built, or every page in `--browser`')
    parser.add_argument('--scenario', default='gauntlet')
    parser.add_argument('--rounds', type=int, default=3,
                        help='launches of every app; the first is its first launch after its install')
    parser.add_argument('--run', type=float, default=10.0,
                        help='seconds each launch is measured for, from its start command')
    parser.add_argument('--interval', type=float, default=0.5)
    parser.add_argument('--clock-ticks', type=int, default=100)
    parser.add_argument('--extra', default='', help='more `am start` extras')
    parser.add_argument('--install', type=Path, help='folder of APP.apk builds to install first')
    parser.add_argument('--browser', type=Path, metavar='DIR',
                        help='folder of pages `build_apps.sh browser` built, to open in Chrome')
    parser.add_argument('--tier', type=int, default=8,
                        help="the gauntlet's tier in Chrome on the phone: the lightest at which no app reaches "
                             "60 fps and the heaviest still draw 5 (README)")
    parser.add_argument('--main', help="the commit the Cranpose app was built at, recorded with the run")
    parser.add_argument('--release', help='the release tag `cranpose-release` was built at')
    args = parser.parse_args()
    if not args.apps:
        args.apps = ','.join(name for name in versions.BROWSER_APPS if (args.browser / name / 'index.html').exists()
                             ) if args.browser else DEFAULT_APPS
    with device_lock(args.serial):
        run = measure_frameworks(args)
    (args.output / 'frameworks.json').write_text(json.dumps(run, indent=1))
    for app, values in run['scenarios'][0]['summary'].items():
        print(f'{app:10} ' + (f'fps {values["fps"]:5.1f} cpu/f {values["cpu_ms_per_frame"]:6.1f}' if values
                              else 'no legs'))
    print('DURATION', run['duration_s'], 's')


if __name__ == '__main__':
    main()
