import unittest
from unittest.mock import Mock, patch

from desktop_access import LinuxAdapter


class NativeError(Exception):
    def __init__(self, message, domain="atspi_error", code=1):
        super().__init__(message)
        self.message = message
        self.domain = domain
        self.code = code


class LinuxSnapshotTests(unittest.TestCase):
    def adapter(self, effects):
        adapter = LinuxAdapter.__new__(LinuxAdapter)
        adapter.error_type = NativeError
        adapter._nodes = Mock(side_effect=effects)
        return adapter

    @patch("desktop_access.time.sleep")
    def test_removed_object_path_retries_the_complete_snapshot(self, sleep):
        error = NativeError("/org/a11y/atspi/accessible/0/498062089990157893632")
        adapter = self.adapter([error, [{"name": "Restored control"}]])
        self.assertEqual(adapter.nodes(), [{"name": "Restored control"}])
        self.assertEqual(adapter._nodes.call_count, 2)
        sleep.assert_called_once()

    @patch("desktop_access.time.sleep")
    def test_persistent_missing_objects_still_fail(self, sleep):
        error = NativeError("/org/a11y/atspi/accessible/0/1")
        adapter = self.adapter([error, error, error])
        with self.assertRaises(NativeError):
            adapter.nodes()
        self.assertEqual(adapter._nodes.call_count, 3)

    def test_unrelated_native_failures_are_not_retried(self):
        for error in [
            NativeError("Permission denied"),
            NativeError("/org/a11y/atspi/accessible/0/1", domain="other"),
            NativeError("/org/a11y/atspi/accessible/0/1", code=2),
        ]:
            adapter = self.adapter([error])
            with self.assertRaises(NativeError):
                adapter.nodes()
            self.assertEqual(adapter._nodes.call_count, 1)
