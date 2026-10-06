#!/usr/bin/env python3
"""The nightly's framework comparison on the attached phone.

Builds the Cranpose benchmark app at main and, when the latest release draws
the gauntlet, at the release (as `nightly.py` builds them, so the night's
earlier builds are reused), puts both beside the other frameworks' builds
macm3 handed over in APKS, and runs `frameworks.py` over all of them.
Usage: frameworks_phone.py --serial SERIAL --output DIR --apks DIR [--release-tree DIR]
"""

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

from nightly import ANDROID, BENCH, REPO, build, git, release_has, release_tree_at


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apks', required=True, type=Path)
    parser.add_argument('--release-tree', type=Path, default=Path.home() / 'perf-nightly' / 'release-tree')
    args = parser.parse_args()
    main_commit = git('rev-parse', 'HEAD')
    release = git('describe', '--tags', '--abbrev=0', '--match', 'v*')
    apps = ['cranpose']
    shutil.copy(build(REPO), args.apks / 'cranpose.apk')
    if (release_has(release, BENCH / 'measure.py', "'gauntlet': '")
            and release_has(release, ANDROID / 'app/build.gradle.kts', 'perfCompareSuffix')):
        shutil.copy(build(release_tree_at(args.release_tree, release), '.release'),
                    args.apks / 'cranpose-release.apk')
        apps.append('cranpose-release')
    else:
        print(f'{release} draws no gauntlet: no cranpose-release', flush=True)
    apps += sorted(apk.stem for apk in args.apks.glob('*.apk') if apk.stem not in apps)
    subprocess.run([sys.executable, str(REPO / BENCH / 'frameworks.py'), '--serial', args.serial,
                    '--output', str(args.output), '--install', str(args.apks), '--apps', ','.join(apps),
                    '--main', main_commit, '--release', release], check=True)


if __name__ == '__main__':
    main()
