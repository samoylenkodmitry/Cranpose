import copy
import unittest
from unittest.mock import Mock, patch

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
        return {'passedTests': 3, 'failedTests': 0, 'skippedTests': 0,
                'expectedFailures': 0, 'totalTestCount': 3, 'result': 'Passed'}


if __name__ == '__main__':
    unittest.main()
