import copy
import unittest

from desktop_access import NativeAdapter
from desktop_robot import run_checks, wait_for
from ios_robot import verify_summary


class FixtureAdapter(NativeAdapter):
    def __init__(self):
        self.count = 0
        self.volume = 30
        self.text = 'initial'
        self.extra = []
        self.disabled_enabled = False
        self.progress_role = 'progress'

    def nodes(self):
        names = ['Accessibility robot', 'Account, Account', 'Remove', 'Increase', 'Decrease',
                 f'Action count: {self.count}', f'Volume value: {self.volume}', f'Edited: {self.text}']
        return ([{'name': name, 'enabled': True} for name in names] + self.extra
                + [{'name': 'Disabled action', 'enabled': self.disabled_enabled},
                   {'name': 'Loading', 'role': self.progress_role, 'value': 40},
                   {'name': 'Volume', 'role': 'slider', 'value': self.volume},
                   {'name': 'Notes', 'value': self.text}])

    def activate(self, node):
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
    def test_all_native_scenarios_must_execute(self):
        report = {'passed': []}
        run_checks(FixtureAdapter(), report)
        self.assertEqual(len(report['passed']), 5)

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
