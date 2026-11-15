"""`perf_window.sh` starts the app itself, then finds its processes and its
buffer layer on the device, so no adb round trip of the host lies between
the start command and the window's first poll of the layer. The host reads
what the script found, and a launch that failed fails the run."""

import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))

import measure  # noqa: E402

SOURCE = (BENCHMARK / 'perf_window.sh').read_text()


def run_awk(program, variable, value, text):
    """Runs one awk program of `perf_window.sh` over `text`, as the device does."""
    with tempfile.NamedTemporaryFile('w', suffix='.txt', delete=False) as handle:
        handle.write(text)
    result = subprocess.run(['awk', '-v', f'{variable}={value}', program, handle.name],
                            capture_output=True, text=True, check=True)
    Path(handle.name).unlink()
    return result.stdout.splitlines()


def matching_layers(pattern, listing):
    program = re.search(r"""dumpsys SurfaceFlinger --list \| awk -v pattern="\$LAYER_PATTERN" '(.*?)'""",
                        SOURCE, re.S).group(1)
    return run_awk(program, 'pattern', pattern, listing)


def every_process(package, listing):
    program = re.search(r"""ps -A -o PID,NAME \| awk -v package="\$PKG" '(.*?)'""", SOURCE, re.S).group(1)
    return run_awk(program, 'package', package, listing)


# `dumpsys SurfaceFlinger --list` on the Huawei Mate 20 X (Android 10) while
# Cranpose's gauntlet starts beside the release build.
MATE_LAYERS = """Display Overlays#0
Splash Screen dev.perfcompare.cranpose#0
dev.perfcompare.cranpose.release/dev.cranpose.android.CranposeActivity#0
AppWindowToken{86d2048 token=Token{888a90d ActivityRecord{86d2105 u0 dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity t4227}}}#0
dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity#0
SurfaceView - dev.perfcompare.egui/android.app.NativeActivity#0
dev.perfcompare.egui/android.app.NativeActivity#0
"""


class LayerTest(unittest.TestCase):
    def test_an_app_window_is_found_by_its_package_alone(self):
        self.assertEqual(matching_layers(measure.app_layer_pattern('cranpose'), MATE_LAYERS),
                         ['dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity#0'])
        self.assertEqual(matching_layers(measure.app_layer_pattern('cranpose-release'), MATE_LAYERS),
                         ['dev.perfcompare.cranpose.release/dev.cranpose.android.CranposeActivity#0'])

    def test_an_app_that_draws_into_a_surface_view_is_found_by_its_surface(self):
        self.assertEqual(matching_layers(measure.app_layer_pattern('egui'), MATE_LAYERS),
                         ['SurfaceView - dev.perfcompare.egui/android.app.NativeActivity#0'])

    def test_the_layer_newer_android_gives_the_window_buffers_is_found(self):
        listing = ('RequestedLayerState{dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity#81 '
                   'parentId=80}\n'
                   'RequestedLayerState{VRI-dev.perfcompare.compose/dev.perfcompare.compose.MainActivity#90 '
                   'parentId=89}\n')
        self.assertEqual(matching_layers(measure.app_layer_pattern('compose'), listing),
                         ['VRI-dev.perfcompare.compose/dev.perfcompare.compose.MainActivity#90'])

    def test_a_page_surface_named_by_its_layer_id_is_found_once_it_is_there(self):
        pattern = measure.PageTarget.layer_pattern
        before = 'RequestedLayerState{com.android.chrome/com.google.android.apps.chrome.Main#62777 parentId=1}\n'
        self.assertEqual(matching_layers(pattern, before), [])
        shown = before + 'RequestedLayerState{com.android.chrome/ChromeChildSurface#62791 parentId=62789}\n'
        self.assertEqual(matching_layers(pattern, shown), ['com.android.chrome/ChromeChildSurface#62791'])

    def test_the_older_page_surface_name_is_found_as_before(self):
        self.assertEqual(matching_layers(measure.PageTarget.layer_pattern, 'com.android.chrome/ChromeChildSurface#0\n'),
                         ['com.android.chrome/ChromeChildSurface#0'])


class ProcessTest(unittest.TestCase):
    LISTING = """  PID NAME
 1201 com.android.chrome_zygote
 1302 com.android.chrome:sandboxed_process0:org.chromium.content.app.SandboxedProcessService0:0
 1250 com.android.chrome
 1311 com.android.chrome:privileged_process0
"""

    def test_the_browser_comes_first_and_its_zygote_is_left_out(self):
        self.assertEqual(every_process('com.android.chrome', self.LISTING), ['1250 1302 1311'])

    def test_helpers_without_the_browser_are_not_its_processes_yet(self):
        listing = '\n'.join(line for line in self.LISTING.splitlines() if not line.endswith('chrome'))
        self.assertEqual(every_process('com.android.chrome', listing), [])


STARTED = """LAUNCH 891131.43
Starting: Intent { cmp=dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity (has extras) }
Status: ok
LaunchState: COLD
Activity: dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity
TotalTime: 158
WaitTime: 162
Complete
"""


class WindowStartTest(unittest.TestCase):
    def test_the_start_is_read_with_the_device_clock_of_the_start_command(self):
        output = STARTED + 'PIDS 27036\nLAYER dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity#0\nPU 1\n'
        times, pids, layer = measure.window_start(output, measure.AppTarget('cranpose'))
        self.assertEqual(times, {'TotalTime': 158, 'WaitTime': 162, 'uptime_s': 891131.43})
        self.assertEqual(pids, [27036])
        self.assertEqual(layer, 'dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity#0')

    def test_a_start_the_system_refused_fails(self):
        output = ('LAUNCH 1.0\nError: Activity class {dev.perfcompare.maui/.MainActivity} does not exist.\n'
                  'LAUNCH_FAILED\n')
        with self.assertRaisesRegex(RuntimeError, 'launch failed'):
            measure.window_start(output, measure.AppTarget('maui'))

    def test_an_app_without_its_layer_fails(self):
        with self.assertRaisesRegex(ValueError, 'expected one buffer layer'):
            measure.window_start(STARTED + 'NO_LAYER\n', measure.AppTarget('cranpose'))

    def test_an_app_with_two_layers_fails_and_a_page_measures_the_first(self):
        found = 'PIDS 1250 1302\nLAYER com.android.chrome/ChromeChildSurface#1\nLAYER com.android.chrome/ChromeChildSurface#2\n'
        with self.assertRaisesRegex(ValueError, 'expected one buffer layer'):
            measure.window_start(STARTED + found, measure.AppTarget('cranpose'))
        _, pids, layer = measure.window_start(STARTED + found, measure.PageTarget(None, 'cranpose', 8))
        self.assertEqual((pids, layer), ([1250, 1302], 'com.android.chrome/ChromeChildSurface#1'))


class FullHistoryTest(unittest.TestCase):
    def test_a_first_poll_whose_newest_presents_are_pending_is_full(self):
        # The Mate's late legs of 2026-11-15: 126 presents and two pending.
        rows = [f'{i} {1_000_000_000 + i * 16_666_667} {i}' for i in range(1, 127)]
        rows += [f'{i} {measure.PENDING} {i}' for i in (127, 128)]
        poll = 'LAT_BEGIN 1.0\n16666667\n' + '\n'.join(rows) + '\nLAT_END\n'
        self.assertTrue(measure.first_poll_full(poll))


if __name__ == '__main__':
    unittest.main()
