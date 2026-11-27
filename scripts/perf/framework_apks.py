#!/usr/bin/env python3
"""Add Cranpose main and release to the shared Android benchmark APKs."""

import argparse
import shutil
from pathlib import Path

from nightly import ANDROID, BENCH, REPO, build, git, release_has, release_tree_at


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apks', required=True, type=Path)
    parser.add_argument('--release-tree', type=Path, default=Path.home() / 'perf-nightly' / 'release-tree')
    args = parser.parse_args()
    release = git('describe', '--tags', '--abbrev=0', '--match', 'v*')
    shutil.copy(build(REPO), args.apks / 'cranpose.apk')
    if (release_has(release, BENCH / 'measure.py', "'gauntlet': '")
            and release_has(release, ANDROID / 'app/build.gradle.kts', 'perfCompareSuffix')):
        shutil.copy(build(release_tree_at(args.release_tree, release), '.release'), args.apks / 'cranpose-release.apk')
    else:
        print(f'{release} draws no gauntlet: no cranpose-release', flush=True)


if __name__ == '__main__':
    main()
