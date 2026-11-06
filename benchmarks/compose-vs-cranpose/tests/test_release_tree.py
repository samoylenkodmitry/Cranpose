"""`build_apps.sh release TAG` from two runners' checkouts that share one
build cache, as macm3's runners do. The release tree belongs to the checkout
that made it. A newer tag that only the other runner fetched still checks
out in it."""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

BENCHMARK = Path(__file__).resolve().parents[1]
APP = 'benchmarks/compose-vs-cranpose'


class ReleaseTreeTest(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.root = Path(folder.name)
        self.env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1',
                        GIT_AUTHOR_NAME='t', GIT_AUTHOR_EMAIL='t@t', GIT_COMMITTER_NAME='t',
                        GIT_COMMITTER_EMAIL='t@t', PERF_BUILD_CACHE=str(self.root / 'cache'),
                        ANDROID_HOME=str(self.root))
        self.origin = self.root / 'origin'
        self.git(self.root, 'init', '-q', 'origin')
        (self.origin / APP / 'cranpose-app/src').mkdir(parents=True)
        shutil.copy(BENCHMARK / 'build_apps.sh', self.origin / APP / 'build_apps.sh')
        self.release('v1')

    def git(self, folder, *arguments):
        return subprocess.run(['git', '-C', str(folder), *arguments], check=True, capture_output=True, text=True,
                              env=self.env).stdout.strip()

    def release(self, tag):
        """A commit of the origin tagged `tag`, from before the desktop
        gauntlet: the build checks the release out and stops there."""
        (self.origin / APP / 'cranpose-app/src/lib.rs').write_text(f'// {tag}\n')
        self.git(self.origin, 'add', '-A')
        self.git(self.origin, 'commit', '-q', '-m', tag)
        self.git(self.origin, 'tag', tag)
        return self.git(self.origin, 'rev-parse', 'HEAD')

    def build(self, runner, tag):
        subprocess.run([str(runner / APP / 'build_apps.sh'), 'release', tag], check=True, capture_output=True,
                       env=self.env)
        return self.git(self.root / 'cache/release-tree', 'rev-parse', 'HEAD')

    def test_a_tag_fetched_by_another_runner_checks_out_in_the_tree_of_the_first(self):
        first, second = self.root / 'runner-1', self.root / 'runner-2'
        self.git(self.root, 'clone', '-q', 'origin', 'runner-1')
        self.git(self.root, 'clone', '-q', 'origin', 'runner-2')
        self.assertEqual(self.build(first, 'v1'), self.git(self.origin, 'rev-parse', 'v1'))
        newer = self.release('v2')
        self.git(second, 'fetch', '-q', '--tags')
        self.assertEqual(self.build(second, 'v2'), newer)


if __name__ == '__main__':
    unittest.main()
