import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location('publish', Path(__file__).resolve().parent / 'publish.py')
publish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publish)


def run():
    return {
        'kind': 'nightly',
        'started_at': '2026-10-05T01:30:00+00:00',
        'device': {'ro.product.model': 'EVR-AL00'},
        'main': 'abc123def456',
        'release': 'v0.9.7',
        'subjects': [{'name': 'cranpose-release', 'label': 'v0.9.7', 'package': 'x'},
                     {'name': 'cranpose', 'label': 'abc123def', 'package': 'y'}],
        'duration_s': 420,
        'scenarios': [{
            'scenario': 'gauntlet',
            'legs': [{}, {}, {}, {}],
            'summary': {'cranpose-release': {'fps': 50.0, 'cpu_ms_per_frame': 33.0, 'gpu_mhz': 139,
                                             'interval_p99_ms': 41.0},
                        'cranpose': {'fps': 52.8, 'cpu_ms_per_frame': 32.0, 'gpu_mhz': 139,
                                     'interval_p99_ms': 38.0}},
            'verdicts': {'fps': 'better', 'cpu_ms_per_frame': 'same', 'desired_to_present_p50_ms': 'same'},
        }],
        'confirmed_regressions': {},
    }


class IndexEntryTest(unittest.TestCase):
    def test_an_entry_carries_each_scenarios_medians_and_verdicts_without_its_legs(self):
        entry = publish.index_entry(run(), 'runs/x.json')
        gauntlet = entry['scenarios']['gauntlet']
        self.assertEqual(gauntlet['legs'], 4)
        self.assertEqual(gauntlet['summary']['cranpose']['fps'], 52.8)
        self.assertEqual(gauntlet['summary']['cranpose']['gpu_mhz'], 139)
        self.assertNotIn('interval_p99_ms', gauntlet['summary']['cranpose'], 'only the charted metrics')
        self.assertEqual(gauntlet['verdicts']['fps'], 'better')
        self.assertEqual(entry['device'], 'EVR-AL00')
        self.assertEqual([subject['label'] for subject in entry['subjects']], ['v0.9.7', 'abc123def'])


# The scratch repositories take nothing from the machine's git: not a hook's
# repository a test run inherits, nor global signing or hooks.
ENV = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
ENV.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1', GIT_AUTHOR_NAME='test',
           GIT_AUTHOR_EMAIL='test@example.com', GIT_COMMITTER_NAME='test', GIT_COMMITTER_EMAIL='test@example.com')

# Refuses the first push it receives, as GitHub did with an Internal Server
# Error on 2026-10-07.
REFUSE_ONCE = """#!/bin/sh
if [ ! -e "$GIT_DIR/refused" ]; then touch "$GIT_DIR/refused"; echo "Internal Server Error" >&2; exit 1; fi
"""


def git(*args, cwd):
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True, text=True,
                          env=ENV).stdout.strip()


class PublishPushTest(unittest.TestCase):
    def test_a_refused_push_is_made_again_on_the_latest_data_commit(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo = root / 'origin.git', root / 'repo'
            git('init', '-q', '--bare', str(origin), cwd=root)
            git('init', '-q', '-b', 'main', str(repo), cwd=root)
            (repo / 'scripts/perf').mkdir(parents=True)
            shutil.copy(Path(__file__).resolve().parent / 'publish.py', repo / 'scripts/perf/publish.py')
            git('add', '-A', cwd=repo)
            git('commit', '-q', '-m', 'start', cwd=repo)
            git('remote', 'add', 'origin', str(origin), cwd=repo)
            git('push', '-q', 'origin', 'main', cwd=repo)
            # The data branch already holds an earlier run.
            git('checkout', '-q', '--orphan', 'perf-data', cwd=repo)
            git('rm', '-q', '-r', '-f', '--cached', '.', cwd=repo)
            earlier = {'file': 'runs/earlier.json', 'started_at': '2026-10-04T01:30:00+00:00'}
            (repo / 'index.json').write_text(json.dumps({'runs': [earlier]}))
            git('add', 'index.json', cwd=repo)
            git('commit', '-q', '-m', 'earlier', cwd=repo)
            git('push', '-q', 'origin', 'perf-data', cwd=repo)
            git('checkout', '-q', '-f', 'main', cwd=repo)
            hook = origin / 'hooks/pre-receive'
            hook.write_text(REFUSE_ONCE)
            hook.chmod(0o755)
            (root / 'run.json').write_text(json.dumps(run()))
            published = subprocess.run(
                ['python3', 'scripts/perf/publish.py', '--run', str(root / 'run.json'), '--tree', str(root / 'tree')],
                cwd=repo, capture_output=True, text=True, env=ENV)
            self.assertEqual(published.returncode, 0, published.stderr)
            self.assertIn('Internal Server Error', published.stdout)
            index = json.loads(git('show', 'perf-data:index.json', cwd=origin))
            self.assertEqual([entry['file'] for entry in index['runs']],
                             ['runs/earlier.json', 'runs/2026-10-05-nightly-abc123def-EVR-AL00.json'])


if __name__ == '__main__':
    unittest.main()
