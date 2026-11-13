import contextlib
import copy
import io
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import ios_robot
from desktop_access import LinuxAdapter, MacAdapter, NativeAdapter
from desktop_robot import run_checks, wait_for
from ios_robot import verify_summary


class FixtureAdapter(NativeAdapter):
    def __init__(self):
        self.count = 0
        self.overlap_count = 0
        self.volume = 30
        self.text = 'initial'
        self.extra = []
        self.disabled_enabled = False
        self.progress_role = 'progress'
        self.modal = 0

    def nodes(self):
        if self.modal:
            names = ['Close confirmation'] if self.modal == 2 else ['Close preferences', 'Open confirmation']
            return [{'name': name, 'enabled': True} for name in names]
        names = ['Accessibility robot', 'Account, Account', 'Remove', 'Increase', 'Decrease',
                 'Rear action', 'Front action', f'Overlap count: {self.overlap_count}',
                 'Open preferences', f'Action count: {self.count}', f'Volume value: {self.volume}', f'Edited: {self.text}']
        return ([{'name': name, 'enabled': True} for name in names] + self.extra
                + [{'name': 'Disabled action', 'enabled': self.disabled_enabled},
                   {'name': 'Loading', 'role': self.progress_role, 'value': 40},
                   {'name': 'Volume', 'role': 'slider', 'value': self.volume},
                   {'name': 'Notes', 'value': self.text}])

    def activate(self, node):
        if node['name'] in ['Rear action', 'Front action']:
            self.overlap_count += 1 if node['name'] == 'Rear action' else 10
            return True
        modal = {'Open preferences': 1, 'Open confirmation': 2, 'Close confirmation': 1, 'Close preferences': 0}
        if node['name'] in modal:
            self.modal = modal[node['name']]
            return True
        delta = {'Increase': 1, 'Remove': 1, 'Decrease': -1}.get(node['name'])
        if delta is None:
            return False
        self.count += delta
        return True

    def set_value(self, node, value):
        if node['name'] == 'Notes':
            self.text = value
        elif node['name'] == 'Volume':
            self.volume = value
        else:
            return False
        return True


class DesktopContractTests(unittest.TestCase):
    def test_linux_retries_only_transient_removed_nodes(self):
        for message, recover, attempts in [
                ("Unknown object '/org/a11y/atspi/accessible/removed'", True, 2),
                ("Unknown object '/org/a11y/atspi/accessible/removed'", False, 3),
                ('Permission denied', False, 1)]:
            with self.subTest(message=message, recover=recover):
                adapter = LinuxAdapter.__new__(LinuxAdapter)
                adapter.api = Mock()
                adapter.pid = 42
                adapter.error_type = RuntimeError
                error = RuntimeError(message)
                error.message = message
                error.domain = 'atspi_error'
                error.code = 1
                desktop = Mock()
                desktop.get_child_count.return_value = 0
                adapter.api.get_desktop.side_effect = [error, desktop] if recover else error
                if recover:
                    self.assertEqual(adapter.nodes(), [])
                else:
                    with self.assertRaises(RuntimeError):
                        adapter.nodes()
                self.assertEqual(adapter.api.get_desktop.call_count, attempts)

    def test_macos_queries_have_a_configurable_timeout(self):
        api = Mock()
        api.AXIsProcessTrusted.return_value = True
        api.AXUIElementSetMessagingTimeout.return_value = 0
        with patch.dict('sys.modules', {'ApplicationServices': api}):
            MacAdapter(42, timeout=2.0)
            api.AXUIElementSetMessagingTimeout.assert_called_once_with(
                api.AXUIElementCreateSystemWide.return_value, 2.0)
            api.AXUIElementSetMessagingTimeout.return_value = -1
            with self.assertRaisesRegex(RuntimeError, 'accessibility timeout: -1'):
                MacAdapter(42)

    def test_all_native_scenarios_must_execute(self):
        report = {'passed': []}
        run_checks(FixtureAdapter(), report)
        self.assertEqual(len(report['passed']), 7)

    def test_rejects_sensitive_text_disabled_actions_and_wrong_roles(self):
        for attribute, value, message in [
                ('extra', [{'name': 'robot-secret-value'}], 'private text'),
                ('extra', [{'name': 'Decorative secret'}], 'private text'),
                ('disabled_enabled', True, 'reported enabled'),
                ('progress_role', 'slider', 'passive progress role')]:
            with self.subTest(attribute=attribute, value=value):
                adapter = FixtureAdapter()
                setattr(adapter, attribute, value)
                with self.assertRaisesRegex(AssertionError, message):
                    run_checks(adapter, {'passed': []})

    def test_missing_native_nodes_fail_with_a_deadline(self):
        with self.assertRaisesRegex(AssertionError, 'Timed out'):
            wait_for(lambda: None, 'missing node', timeout=0)

    def test_rejects_modal_background_controls(self):
        adapter = FixtureAdapter()
        nodes = adapter.nodes
        adapter.nodes = lambda: nodes() + ([{'name': 'Increase'}] if adapter.modal else [])
        with self.assertRaisesRegex(AssertionError, 'modal exposes background control Increase'):
            run_checks(adapter, {'passed': []})

    def test_every_desktop_platform_rejects_a_disabled_control_reported_enabled(self):
        for platform in ['linux', 'darwin', 'win32']:
            with self.subTest(platform=platform):
                adapter = FixtureAdapter()
                adapter.disabled_enabled = True
                with self.assertRaisesRegex(AssertionError, 'reported enabled'):
                    run_checks(adapter, {'platform': platform, 'passed': []})

    def test_every_desktop_platform_passes_a_disabled_control_reported_disabled(self):
        for platform in ['linux', 'darwin', 'win32']:
            with self.subTest(platform=platform):
                report = {'platform': platform, 'passed': []}
                run_checks(FixtureAdapter(), report)
                self.assertEqual(len(report['passed']), 7)

    def test_rejects_a_disabled_action_that_claims_success(self):
        adapter = FixtureAdapter()
        activate = adapter.activate
        adapter.activate = lambda node: node['name'] == 'Disabled action' or activate(node)
        with self.assertRaisesRegex(AssertionError, 'accepted activation'):
            run_checks(adapter, {'passed': []})

    def test_rejects_missing_native_setters(self):
        adapter = FixtureAdapter()
        adapter.set_value = lambda node, value: False
        with self.assertRaisesRegex(AssertionError, 'range adjustment failed'):
            run_checks(adapter, {'passed': []})


class IosContractTests(unittest.TestCase):
    def test_accepts_complete_run(self):
        verify_summary(self.summary())

    def test_rejects_partial_skipped_failed_or_expected_failures(self):
        for key, value in [('passedTests', 2), ('failedTests', 1), ('skippedTests', 1),
                           ('expectedFailures', 1), ('totalTestCount', 0), ('result', 'Failed')]:
            with self.subTest(key=key):
                summary = copy.copy(self.summary())
                summary[key] = value
                with self.assertRaises(AssertionError):
                    verify_summary(summary)
        with self.assertRaises(AssertionError):
            verify_summary({})

    @staticmethod
    def summary():
        return {'passedTests': 4, 'failedTests': 0, 'skippedTests': 0,
                'expectedFailures': 0, 'totalTestCount': 4, 'result': 'Passed'}


class IosRepeatTests(unittest.TestCase):
    """The robot's main() with each xcodebuild run replaced by a canned result."""

    FAILURE = {'failureText': 'Failed to launch io.cranpose.demo: Timed out attempting to launch app.'}

    def failed_summary(self):
        return dict(IosContractTests.summary(), passedTests=2, failedTests=2, result='Failed',
                    testFailures=[self.FAILURE])

    @staticmethod
    def run_main(runs):
        """Runs main() with `runs`, each an (exit status, summary) pair, as xcodebuild's runs in order.
        Returns the report, the number of runs that happened and the error main raised, if any."""
        with tempfile.TemporaryDirectory() as root:
            app = Path(root) / 'Demo.app'
            app.mkdir()
            (app / 'Info.plist').write_bytes(plistlib.dumps(
                {'CFBundleExecutable': 'Demo', 'CFBundleIdentifier': 'io.cranpose.demo'}))
            (app / 'Demo').write_bytes(b'binary')
            output = Path(root) / 'out'
            canned = [(subprocess.CompletedProcess([], status), summary) for status, summary in runs]
            argv = ['ios_robot.py', '--app', str(app), '--device', 'UDID', '--output', str(output)]
            raised = None
            with patch.object(sys, 'argv', argv), patch.object(ios_robot, 'boot'), \
                    patch.object(ios_robot, 'command', return_value=''), \
                    patch.object(ios_robot, 'run_tests', side_effect=canned) as run_tests, \
                    contextlib.redirect_stdout(io.StringIO()):
                try:
                    ios_robot.main()
                except subprocess.CalledProcessError as error:
                    raised = error
            return json.loads((output / 'report.json').read_text()), run_tests.call_count, raised

    def test_a_passing_run_is_not_repeated(self):
        report, runs, raised = self.run_main([(0, IosContractTests.summary())])
        self.assertIsNone(raised)
        self.assertEqual((report['status'], runs), ('passed', 1))
        self.assertNotIn('earlier_failures', report)

    def test_a_failed_run_is_repeated_once_and_its_failures_are_kept(self):
        report, runs, raised = self.run_main([(65, self.failed_summary()), (0, IosContractTests.summary())])
        self.assertIsNone(raised)
        self.assertEqual((report['status'], runs), ('passed', 2))
        self.assertEqual(report['earlier_failures'], [[self.FAILURE]])

    def test_two_failed_runs_fail_the_robot(self):
        report, runs, raised = self.run_main([(65, self.failed_summary()), (65, self.failed_summary())])
        self.assertIsInstance(raised, subprocess.CalledProcessError)
        self.assertEqual((report['status'], runs), ('failed', 2))

    def test_a_run_that_exits_cleanly_with_a_skipped_test_is_repeated(self):
        skipped = dict(IosContractTests.summary(), passedTests=3, skippedTests=1)
        report, runs, raised = self.run_main([(0, skipped), (0, IosContractTests.summary())])
        self.assertIsNone(raised)
        self.assertEqual((report['status'], runs), ('passed', 2))


class IosPrepareTests(unittest.TestCase):
    """The robot's boot and install with simctl replaced: `hangs` installs time out before one works."""

    @staticmethod
    def prepare(hangs):
        installs = []

        def simctl(*args, **kwargs):
            if args[2] == 'install':
                installs.append(kwargs.get('timeout'))
                if len(installs) <= hangs:
                    raise subprocess.TimeoutExpired(list(args), kwargs.get('timeout'))
            return ''

        report = {}
        raised = None
        with patch.object(ios_robot, 'boot') as boot, \
                patch.object(ios_robot, 'command', side_effect=simctl), \
                patch.object(ios_robot.subprocess, 'run') as shutdown:
            try:
                ios_robot.prepare('UDID', Path('Demo.app'), report)
            except subprocess.TimeoutExpired as error:
                raised = error
        return report, installs, boot.call_count, shutdown.call_count, raised

    def test_an_install_that_works_at_once_boots_once(self):
        report, installs, boots, shutdowns, raised = self.prepare(hangs=0)
        self.assertIsNone(raised)
        self.assertEqual((report['prepare_attempts'], installs, boots, shutdowns),
                         (1, [ios_robot.STEP_SECONDS], 1, 0))

    def test_a_hung_install_shuts_the_simulator_down_and_boots_it_again(self):
        report, installs, boots, shutdowns, raised = self.prepare(hangs=1)
        self.assertIsNone(raised)
        self.assertEqual((report['prepare_attempts'], len(installs), boots, shutdowns), (2, 2, 2, 1))

    def test_a_hang_in_every_attempt_fails_the_robot(self):
        report, installs, boots, shutdowns, raised = self.prepare(hangs=ios_robot.PREPARE_ATTEMPTS)
        self.assertIsInstance(raised, subprocess.TimeoutExpired)
        self.assertNotIn('prepare_attempts', report)
        self.assertEqual((len(installs), shutdowns), (ios_robot.PREPARE_ATTEMPTS, ios_robot.PREPARE_ATTEMPTS - 1))


if __name__ == '__main__':
    unittest.main()
