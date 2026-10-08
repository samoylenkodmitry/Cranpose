#!/usr/bin/env python3
"""`propose_versions.py` in a scratch repository with a stand-in `gh`: it
commits the moved pins' app folders and nothing the night's builds changed
elsewhere."""

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / 'propose_versions.py'
BENCHMARK = 'benchmarks/compose-vs-cranpose'


def git(*args, cwd):
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True, text=True,
                          env=ENV).stdout.strip()


# The scratch repositories take nothing from the machine's git: not a hook's
# repository a test run inherits, nor global signing or hooks.
ENV = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
ENV.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')


class ProposeVersionsTest(unittest.TestCase):
    def test_only_the_moved_pins_folders_are_proposed(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo, tools = root / 'origin.git', root / 'repo', root / 'bin'
            git('init', '-q', '--bare', str(origin), cwd=root)
            git('init', '-q', '-b', 'main', str(repo), cwd=root)
            git('config', 'user.name', 'test', cwd=repo)
            git('config', 'user.email', 'test@example.com', cwd=repo)
            (repo / 'scripts/perf').mkdir(parents=True)
            shutil.copy(SCRIPT, repo / 'scripts/perf/propose_versions.py')
            for path, text in {f'{BENCHMARK}/nativescript-app/package.json': '"9.1.2"',
                               f'{BENCHMARK}/nativescript-app/package-lock.json': 'old lock',
                               f'{BENCHMARK}/flutter-app/pubspec.lock': 'flutter lock'}.items():
                (repo / path).parent.mkdir(parents=True, exist_ok=True)
                (repo / path).write_text(text)
            git('add', '-A', cwd=repo)
            git('commit', '-q', '-m', 'start', cwd=repo)
            git('remote', 'add', 'origin', str(origin), cwd=repo)
            git('push', '-q', 'origin', 'main', cwd=repo)
            # What the night did: the pin and its lock moved, and a build
            # rewrote another app's lock.
            (repo / BENCHMARK / 'nativescript-app/package.json').write_text('"9.1.3"')
            (repo / BENCHMARK / 'nativescript-app/package-lock.json').write_text('new lock')
            (repo / BENCHMARK / 'flutter-app/pubspec.lock').write_text('pruned by a build')
            record = root / 'bumps.json'
            record.write_text(json.dumps({'nativescript': {
                'from': '9.1.2', 'to': '9.1.3', 'files': ['nativescript-app/package.json'], 'reverted': False}}))
            tools.mkdir()
            # A Mac whose git signs every commit with a key only a person can unlock.
            machine = Path(folder) / 'machine-gitconfig'
            machine.write_text('[commit]\n\tgpgsign = true\n[gpg]\n\tprogram = false\n')
            (tools / 'gh').write_text('#!/bin/sh\ncase "$2" in create) echo https://example.com/pull/1;; esac\n')
            (tools / 'gh').chmod(0o755)
            subprocess.run(['python3', 'scripts/perf/propose_versions.py', '--record', str(record)], cwd=repo,
                           check=True, capture_output=True, text=True,
                           env=dict(ENV, PATH=f'{tools}{os.pathsep}{os.environ["PATH"]}', GIT_CONFIG_GLOBAL=str(machine)))
            proposed = git('diff', '--name-only', 'main', 'origin/perf/framework-versions', cwd=repo).splitlines()
            self.assertEqual(sorted(proposed), [f'{BENCHMARK}/nativescript-app/package-lock.json',
                                                f'{BENCHMARK}/nativescript-app/package.json'])


if __name__ == '__main__':
    unittest.main()
