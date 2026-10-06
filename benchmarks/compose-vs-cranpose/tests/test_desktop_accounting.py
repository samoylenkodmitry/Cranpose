import importlib.util
import sys
from pathlib import Path
import unittest


BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('desktop', BENCHMARK / 'desktop.py')
desktop = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(desktop)

WEBCONTENT = '/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/' \
             'com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent'


class DesktopAccountingTest(unittest.TestCase):
    def test_ps_times_read_with_days_hours_and_fractions(self):
        self.assertEqual(desktop.seconds('1-02:03:04'), 93_784.0)
        self.assertEqual(desktop.seconds('12:03:04'), 43_384.0)
        self.assertEqual(desktop.seconds('03:04.50'), 184.5)

    def test_an_app_owns_its_children_and_the_webkit_services_started_for_it(self):
        # pid: (parent, cpu seconds, seconds since start, command)
        table = {
            100: (1, 4.0, 10.0, '/apps/perf-compare-tauri'),
            101: (100, 1.0, 9.0, '/apps/helper'),
            200: (1, 30.0, 8.0, WEBCONTENT),
            300: (1, 50.0, 3600.0, WEBCONTENT),
            400: (1, 2.0, 5.0, '/usr/bin/other'),
        }
        self.assertEqual(desktop.tree(100, table, 11.0), {100, 101, 200})


if __name__ == '__main__':
    unittest.main()
