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


# Stands in for git on the path: a push writes down the configuration the
# environment hands it, then the real git runs.
RECORD_PUSH = """#!/bin/sh
if [ "$1" = push ]; then env | grep -E '^GIT_CONFIG_(COUNT|KEY|VALUE)' > "$PUSH_RECORD"; fi
exec "$REAL_GIT" "$@"
"""


def scratch_repo(root):
    """A clone of this script's repository with the data branch already
    holding an earlier run, and the bare origin it pushes to."""
    origin, repo = root / 'origin.git', root / 'repo'
    git('init', '-q', '--bare', str(origin), cwd=root)
    git('init', '-q', '-b', 'main', str(repo), cwd=root)
    (repo / 'scripts/perf').mkdir(parents=True)
    shutil.copy(Path(__file__).resolve().parent / 'publish.py', repo / 'scripts/perf/publish.py')
    git('add', '-A', cwd=repo)
    git('commit', '-q', '-m', 'start', cwd=repo)
    git('remote', 'add', 'origin', str(origin), cwd=repo)
    git('push', '-q', 'origin', 'main', cwd=repo)
    git('checkout', '-q', '--orphan', 'perf-data', cwd=repo)
    git('rm', '-q', '-r', '-f', '--cached', '.', cwd=repo)
    earlier = {'file': 'runs/earlier.json', 'started_at': '2026-10-04T01:30:00+00:00'}
    (repo / 'index.json').write_text(json.dumps({'runs': [earlier]}))
    git('add', 'index.json', cwd=repo)
    git('commit', '-q', '-m', 'earlier', cwd=repo)
    git('push', '-q', 'origin', 'perf-data', cwd=repo)
    git('checkout', '-q', '-f', 'main', cwd=repo)
    (root / 'run.json').write_text(json.dumps(run()))
    return origin, repo


# A Mac whose git signs every commit with a key only a person can unlock.
SIGNING = '[commit]\n\tgpgsign = true\n[gpg]\n\tprogram = false\n'


def publish_run(root, repo, environment=None):
    if environment is None:
        machine = root / 'machine-gitconfig'
        machine.write_text(SIGNING)
        environment = {**ENV, 'GIT_CONFIG_GLOBAL': str(machine)}
    return subprocess.run(
        ['python3', 'scripts/perf/publish.py', '--run', str(root / 'run.json'), '--tree', str(root / 'tree')],
        cwd=repo, capture_output=True, text=True, env=environment)


class PublishPushTest(unittest.TestCase):
    def test_a_refused_push_is_made_again_on_the_latest_data_commit(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo = scratch_repo(root)
            hook = origin / 'hooks/pre-receive'
            hook.write_text(REFUSE_ONCE)
            hook.chmod(0o755)
            published = publish_run(root, repo)
            self.assertEqual(published.returncode, 0, published.stderr)
            self.assertIn('Internal Server Error', published.stdout)
            index = json.loads(git('show', 'perf-data:index.json', cwd=origin))
            self.assertEqual([entry['file'] for entry in index['runs']],
                             ['runs/earlier.json', 'runs/2026-10-05-nightly-abc123def-EVR-AL00.json'])

    def test_a_job_with_a_token_answers_git_with_it_in_place_of_the_machines_helper(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo = scratch_repo(root)
            tools = root / 'tools'
            tools.mkdir()
            (tools / 'git').write_text(RECORD_PUSH)
            (tools / 'git').chmod(0o755)
            record = root / 'push-record'
            machine = root / 'machine-gitconfig'
            machine.write_text(SIGNING + '[credential]\n\thelper = !echo password=the-keychain\n')
            environment = {**ENV, 'PATH': f'{tools}{os.pathsep}{ENV["PATH"]}', 'REAL_GIT': shutil.which('git'),
                           'PUSH_RECORD': str(record), 'GIT_CONFIG_GLOBAL': str(machine)}
            self.assertEqual(publish_run(root, repo, environment).returncode, 0)
            self.assertFalse(record.exists() and record.read_text(), 'a job without a token pushes as the machine does')
            (root / 'run.json').write_text(json.dumps({**run(), 'main': 'def456abc789'}))
            published = publish_run(root, repo, {**environment, 'GH_TOKEN': 'job-token'})
            self.assertEqual(published.returncode, 0, published.stderr)
            # What git is asked for with the push's environment: the job's token.
            pushed = dict(line.split('=', 1) for line in record.read_text().splitlines())
            asked = subprocess.run(['git', 'credential', 'fill'], input='protocol=https\nhost=github.com\n\n',
                                   capture_output=True, text=True, check=True,
                                   env={**environment, **pushed, 'GH_TOKEN': 'job-token'}).stdout
            self.assertIn('username=x-access-token', asked)
            self.assertIn('password=job-token', asked)
            self.assertNotIn('the-keychain', asked)

    def test_a_data_tree_the_checkout_lost_its_record_of_is_made_again(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo = scratch_repo(root)
            self.assertEqual(publish_run(root, repo).returncode, 0)
            # actions/checkout makes the checkout's .git again and the tree,
            # which lives beside it, no longer belongs to any repository.
            shutil.rmtree(repo / '.git/worktrees')
            self.assertNotEqual(subprocess.run(['git', 'status'], cwd=root / 'tree', env=ENV,
                                               capture_output=True).returncode, 0)
            (root / 'run.json').write_text(json.dumps({**run(), 'main': 'def456abc789'}))
            published = publish_run(root, repo)
            self.assertEqual(published.returncode, 0, published.stderr)
            index = json.loads(git('show', 'perf-data:index.json', cwd=origin))
            self.assertEqual(len(index['runs']), 3)

    def test_a_data_tree_left_dirty_by_a_failed_commit_follows_a_branch_that_moved(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo = scratch_repo(root)
            self.assertEqual(publish_run(root, repo).returncode, 0)
            # The commit of a later night failed after it wrote its run, and
            # another job pushed to the branch since.
            (root / 'tree/index.json').write_text(json.dumps({'runs': [{'file': 'runs/stale.json'}]}))
            git('add', 'index.json', cwd=root / 'tree')
            git('clone', '-q', '-b', 'perf-data', str(origin), str(root / 'other'), cwd=root)
            index = json.loads((root / 'other/index.json').read_text())
            index['runs'].append({'file': 'runs/other.json', 'started_at': '2026-10-06T01:30:00+00:00'})
            (root / 'other/index.json').write_text(json.dumps(index))
            git('add', 'index.json', cwd=root / 'other')
            git('commit', '-q', '-m', 'other', cwd=root / 'other')
            git('push', '-q', 'origin', 'perf-data', cwd=root / 'other')
            (root / 'run.json').write_text(json.dumps({**run(), 'main': 'def456abc789'}))
            published = publish_run(root, repo)
            self.assertEqual(published.returncode, 0, published.stderr)
            files = [entry['file'] for entry in json.loads(git('show', 'perf-data:index.json', cwd=origin))['runs']]
            # The index lists runs by start; the stale entry of the failed night is not among them.
            self.assertEqual(files, ['runs/earlier.json', 'runs/2026-10-05-nightly-abc123def-EVR-AL00.json',
                                     'runs/2026-10-05-nightly-def456abc-EVR-AL00.json', 'runs/other.json'])

    def test_a_data_branch_that_cannot_be_fetched_is_not_started_again(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo = scratch_repo(root)
            publish_run(root, repo)
            git('remote', 'set-url', 'origin', str(root / 'gone.git'), cwd=repo)
            (root / 'run.json').write_text(json.dumps({**run(), 'main': 'def456abc789'}))
            published = publish_run(root, repo)
            self.assertNotEqual(published.returncode, 0)
            self.assertIn('git fetch origin perf-data failed', published.stderr)
            self.assertNotIn('already exists', published.stderr)

    def test_a_data_branch_the_remote_lacks_is_started(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            origin, repo = scratch_repo(root)
            git('push', '-q', 'origin', '--delete', 'perf-data', cwd=repo)
            git('branch', '-q', '-D', 'perf-data', cwd=repo)
            published = publish_run(root, repo)
            self.assertEqual(published.returncode, 0, published.stderr)
            index = json.loads(git('show', 'perf-data:index.json', cwd=origin))
            self.assertEqual([entry['file'] for entry in index['runs']],
                             ['runs/2026-10-05-nightly-abc123def-EVR-AL00.json'])


if __name__ == '__main__':
    unittest.main()
