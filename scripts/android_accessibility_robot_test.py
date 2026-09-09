import unittest

from android_accessibility_robot import completed_tests


class InstrumentationResultTest(unittest.TestCase):
    def test_accepts_completed_robot_tests(self):
        self.assertEqual(completed_tests('Time: 2.5\nOK (2 tests)\nINSTRUMENTATION_CODE: -1\n'), 2)

    def test_rejects_empty_failed_and_incomplete_instrumentation(self):
        for result in ['OK (0 tests)\n', 'INSTRUMENTATION_CODE: -1\n',
                       'FAILURES!!!\nTests run: 1,  Failures: 1\n',
                       'OK (1 test)\nINSTRUMENTATION_FAILED: runner crashed\n']:
            with self.subTest(result=result), self.assertRaises(RuntimeError):
                completed_tests(result)


if __name__ == '__main__':
    unittest.main()
