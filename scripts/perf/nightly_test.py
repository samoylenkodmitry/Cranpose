import importlib.util
import os
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('nightly', Path(__file__).resolve().parent / 'nightly.py')
nightly = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(nightly)

# Fails the first time it runs, as Gradle did when another job's build held
# the cache lock, and records every run.
GRADLEW = """#!/bin/sh
echo "$*" >> "$GRADLE_CALLS"
if [ ! -e "$GRADLE_CALLS.failed" ]; then touch "$GRADLE_CALLS.failed"; exit 1; fi
"""


def tree_with(gradlew, folder):
    tree = Path(folder)
    directory = tree / nightly.ANDROID
    directory.mkdir(parents=True)
    script = directory / 'gradlew'
    script.write_text(gradlew)
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    return tree


class BuildTest(unittest.TestCase):
    def test_a_build_that_fails_once_is_made_again(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(nightly, 'BUILD_RETRY_DELAY_S', 0):
            calls = Path(folder) / 'calls'
            with patch.dict(os.environ, {'GRADLE_CALLS': str(calls)}):
                apk = nightly.build(tree_with(GRADLEW, Path(folder) / 'tree'), 'release')
            self.assertEqual(len(calls.read_text().splitlines()), 2)
            self.assertEqual(apk.name, 'app-release.apk')

    def test_a_build_that_keeps_failing_stops_the_night_after_its_attempts(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(nightly, 'BUILD_RETRY_DELAY_S', 0):
            calls = Path(folder) / 'calls'
            failing = '#!/bin/sh\necho "$*" >> "$GRADLE_CALLS"\nexit 1\n'
            with patch.dict(os.environ, {'GRADLE_CALLS': str(calls)}):
                with self.assertRaises(subprocess.CalledProcessError):
                    nightly.build(tree_with(failing, Path(folder) / 'tree'))
            self.assertEqual(len(calls.read_text().splitlines()), nightly.BUILD_ATTEMPTS)


def scratch_git(*args, cwd):
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


class ReleaseTreeTest(unittest.TestCase):
    def test_a_release_tree_a_build_changed_follows_the_next_release(self):
        # A scratch repository takes nothing from the machine's git.
        machine = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
        machine.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1', GIT_AUTHOR_NAME='test',
                       GIT_AUTHOR_EMAIL='test@example.com', GIT_COMMITTER_NAME='test',
                       GIT_COMMITTER_EMAIL='test@example.com')
        with tempfile.TemporaryDirectory() as folder, patch.dict(os.environ, machine, clear=True):
            root = Path(folder)
            repo, tree = root / 'repo', root / 'tree'
            scratch_git('init', '-q', '-b', 'main', str(repo), cwd=root)
            for release in ('v1', 'v2'):
                (repo / 'Cargo.lock').write_text(release)
                scratch_git('add', 'Cargo.lock', cwd=repo)
                scratch_git('commit', '-q', '-m', release, cwd=repo)
                scratch_git('tag', release, cwd=repo)
            with patch.object(sys.modules['publish'], 'REPO', repo):
                nightly.release_tree_at(tree, 'v1')
                # The build of the night rewrote the lock file, as cargo does.
                (tree / 'Cargo.lock').write_text('rewritten by a build')
                nightly.release_tree_at(tree, 'v2')
            self.assertEqual((tree / 'Cargo.lock').read_text(), 'v2')


if __name__ == '__main__':
    unittest.main()
