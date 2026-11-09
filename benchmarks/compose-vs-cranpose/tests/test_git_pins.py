"""`versions.py bump` and the pins that name a commit of a repository: they stay
where they are unless the night asks for them, and then move with the rest."""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('versions', BENCHMARK / 'versions.py')
versions = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(versions)


class GitPinsTest(unittest.TestCase):
    def bump(self, *flags):
        """Runs `versions.py bump` over one crate pin and one git pin, each
        behind its latest, and returns what the record holds and what the pin
        files read afterwards."""
        with tempfile.TemporaryDirectory() as folder:
            crate, commit, record = Path(folder) / 'crate.toml', Path(folder) / 'commit.toml', Path(folder) / 'bumps.json'
            crate.write_text('framework = "1.0.0"\n')
            commit.write_text('git = "https://example.test/repo", rev = "aaaa"\n')
            pins = {
                'crate': versions.Pin([str(crate)], r'framework = "([^"]+)"', lambda: '2.0.0'),
                'commit': versions.Pin([str(commit)], r'rev = "([0-9a-f]+)"', lambda: 'bbbb', git=True),
            }
            argv = ['versions.py', 'bump', '--record', str(record), *flags]
            with patch.object(versions, 'PINS', pins), patch.object(sys, 'argv', argv):
                versions.main()
            return json.loads(record.read_text()), crate.read_text(), commit.read_text()

    def test_a_night_moves_the_crate_pin_and_leaves_the_commit_alone(self):
        record, crate, commit = self.bump()
        self.assertEqual(list(record), ['crate'])
        self.assertIn('2.0.0', crate)
        self.assertIn('"aaaa"', commit)

    def test_a_night_asked_for_git_pins_moves_both_into_one_record(self):
        record, crate, commit = self.bump('--git-pins')
        self.assertEqual(sorted(record), ['commit', 'crate'])
        self.assertEqual(record['commit']['to'], 'bbbb')
        self.assertIn('2.0.0', crate)
        self.assertIn('"bbbb"', commit)


if __name__ == '__main__':
    unittest.main()
