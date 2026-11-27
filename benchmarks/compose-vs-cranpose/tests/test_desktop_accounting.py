import importlib.util
import os
import subprocess
import sys
import time
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch


BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('desktop', BENCHMARK / 'desktop.py')
desktop = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(desktop)

WEBCONTENT = '/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/' \
             'com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent'
HOST_LOCK_WRAPPER = BENCHMARK.parents[1] / 'scripts/ci/with_host_lock.sh'


def host_lock_env(root):
    """The environment with a host lock of its own in `root`."""
    env = {k: v for k, v in os.environ.items() if k != 'CRANPOSE_HOST_LOCK_HELD'}
    env.update(
        CRANPOSE_USE_SCCACHE='0',
        CRANPOSE_HOST_LOCK_FILE=str(root / 'capacity.lock'),
        CRANPOSE_HOST_LOCK_TURNSTILE_FILE=str(root / 'turnstile.lock'),
        CRANPOSE_HOST_LOCK_MAX_WAIT_SECS='10',
    )
    return env


def queue_on_host_lock(mode, entered, env):
    """Starts `with_host_lock.sh mode` around a command that creates `entered`.
    Returns the process and whether it waits for the lock: false when it took
    the lock at once."""
    child = subprocess.Popen(
        [str(HOST_LOCK_WRAPPER), mode, sys.executable, '-c',
         'from pathlib import Path; import sys; Path(sys.argv[1]).touch()', str(entered)],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    for line in iter(child.stdout.readline, ''):
        if 'waiting for' in line:
            return child, True
        if 'host lock: took' in line:
            return child, False
    return child, False


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


class DesktopClocksTest(unittest.TestCase):
    def test_a_line_macmon_left_unfinished_when_stopped_is_not_a_sample(self):
        clocks = desktop.Clocks.__new__(desktop.Clocks)
        clocks.started = 10.0
        output = ('{"pcpu_freq_mhz": 4000, "gpu_freq_mhz": 500}\n'
                  '{"pcpu_freq_mhz": 4100, "gpu_freq_mhz": 700}\n'
                  '{"pcpu_freq_mhz": 41')
        clocks.process = SimpleNamespace(terminate=lambda: None, communicate=lambda timeout: (output, None))
        summary, timed = clocks.stop()
        self.assertEqual(summary, {'cpu_mhz': 4050, 'gpu_mhz': 600})
        self.assertEqual(timed, [(10.25, 4000, 500), (10.5, 4100, 700)])

    def test_each_interval_reads_the_cores_cpu_per_frame_footprint_and_last_clocks_since_the_one_before(self):
        mb = desktop.MB
        # (monotonic time, CPU seconds of the app and the window server, the app's footprint)
        taken = [(10.0, 5.0, 0), (10.5, 5.4, 100 * mb), (11.0, 5.4, 120 * mb), (11.5, 5.2, 90 * mb)]
        clocks = [(10.25, 3000, 400), (10.75, 3500, 600)]
        frames = [10.1, 10.2, 10.3, 10.4, 10.6]
        samples = desktop.run_samples(taken, clocks, 10.0, frames)
        self.assertEqual(samples['t_ms'], [500, 1000, 1500])
        self.assertEqual(samples['cores'], [0.8, 0.0, 0.0], 'a process that ended reads as no CPU, not less')
        self.assertEqual(samples['cpu_ms_per_frame'], [100.0, 0.0, None])
        self.assertEqual(samples['ram_mb'], [100.0, 120.0, 90.0])
        self.assertEqual(samples['cpu_mhz'], [3000, 3500, 3500])
        self.assertEqual(samples['gpu_mhz'], [400, 600, 600])


class DesktopRoundsTest(unittest.TestCase):
    """`run` on legs a stand-in for `measure` returns, as the run would read
    `FrameCount`'s rates, with the apps' state cleared by a stand-in that
    notes each clearing."""

    def rounds(self, rates, rounds=1, browser=None, app='fyne', idle=0.5, apps=None, screens=(('gauntlet', 16),),
               max_idle=desktop.MAX_IDLE_OTHERS, max_idle_wait=desktop.MAX_IDLE_WAIT_S, host_lock=False):
        """A rate is a leg, or a (rate, cores other processes spent) pair; a
        `RuntimeError` is an attempt that failed. Each round measures the
        app's first start, then its second. With several `screens` the whole
        report comes back."""
        measured = []
        self.screens = []
        self.cleared = []
        self.programs = []
        self.warmed = []
        self.idle_calls = 0

        def warm_chrome(args, server, stage):
            self.warmed.append(len(measured))

        def install(name, stage, label):
            self.programs.append(f'install {label}')
            return f'installs/{label}/{name}'

        def wait_for_checks(stage):
            self.programs.append('wait')
            return 2.5

        def read_idle(span_s=desktop.IDLE_SPAN_S):
            self.idle_calls += 1
            if callable(idle):
                return idle(span_s)
            if isinstance(idle, (list, tuple)):
                return idle[min(self.idle_calls - 1, len(idle) - 1)]
            return idle

        def measure(name, args, work, page, stage, label, earlier, profile, program=None):
            measured.append(label)
            self.screens.append((args.scenario, args.tier, work))
            self.programs.append(program)
            rate = rates[len(measured) - 1]
            if isinstance(rate, Exception):
                raise rate
            rate, others = rate if isinstance(rate, tuple) else (rate, 0.5)
            return {'fps': rate, 'frames': 90, 'run_s': 3.0, 'first_frame_s': 0.4,
                    'frame_ms': [400.0, 416.7, 433.3], 'fps_by_second': [rate / 2, rate, rate],
                    'interval_p50_ms': 16.7,
                    'interval_p99_ms': 20.0, 'cpu_cores': 1.0, 'server_cores': 0.2, 'other_cores': others,
                    'cpu_ms_per_frame': 40.0}

        def clear(profile):
            self.cleared.append((len(measured), profile.name))

        args = SimpleNamespace(parity=False, output=Path('results'), rounds=rounds, max_others=1.5,
                               max_idle=max_idle, max_idle_wait=max_idle_wait, host_lock=host_lock,
                               main=None, release=None, tier=16, run=3.0, browser=browser, screens=list(screens))
        with patch.object(desktop, 'measure', measure), patch.object(desktop, 'clear_app_state', clear), \
                patch.object(desktop, 'app_bytes', return_value=2048), \
                patch.object(desktop, 'fresh_install', install), \
                patch.object(desktop, 'wait_for_program_checks', wait_for_checks), \
                patch.object(desktop, 'warm_chrome', warm_chrome), \
                patch.object(desktop, 'idle_others', read_idle), \
                patch.object(desktop.subprocess, 'run', return_value=SimpleNamespace(stdout='Apple M3 Pro')):
            report = desktop.run(args, apps or [app], 'page', Path('stage'))
        return measured, report['scenarios'][0] if browser is None and len(screens) == 1 else report

    def test_each_screen_measures_the_apps_that_draw_it_in_a_folder_of_its_own(self):
        with tempfile.TemporaryDirectory() as folder:
            with patch.object(desktop, 'RELEASE_SCREENS', Path(folder) / 'screens.txt'):
                _, report = self.rounds([24.3, 30.0, 24.6, 30.5, 20.0, 22.0], apps=['egui', 'cranpose-release'],
                                        screens=[('gauntlet', 16), ('wall', 20)])
        gauntlet, wall = report['scenarios']
        self.assertEqual((gauntlet['scenario'], wall['scenario']), ('gauntlet', 'wall'))
        self.assertEqual({leg['subject'] for leg in gauntlet['legs']}, {'egui', 'cranpose-release'})
        self.assertEqual({leg['subject'] for leg in wall['legs']}, {'egui'},
                         'a release that knows no wall would show another screen under its name')
        self.assertEqual(self.screens, [('gauntlet', 16, Path('results/gauntlet'))] * 4
                         + [('wall', 20, Path('results/wall'))] * 2)
        self.assertTrue(wall['extras'].startswith('tier 20, '))
        self.assertEqual(set(gauntlet['app_bytes']), {'egui', 'cranpose-release'})
        self.assertEqual(set(wall['app_bytes']), {'egui'})
        self.assertEqual(wall['unimplemented'], ['cranpose-release'])

    def test_each_current_desktop_app_measures_both_starts_on_all_three_screens(self):
        apps = ['cranpose', *desktop.versions.PLATFORM_APPS['desktop']]
        screens = [('gauntlet', 16), ('wall', 20), ('patchwork', 17)]
        with patch.object(desktop.versions, 'version', return_value='test build'):
            _, report = self.rounds([30.0] * (2 * len(screens) * len(apps)), apps=apps, screens=screens)
        for scenario in report['scenarios']:
            self.assertEqual(scenario['unimplemented'], [])
            self.assertEqual(set(scenario['app_bytes']), set(apps))
            for start in ('first', 'second'):
                self.assertEqual(set(scenario['starts'][start]['summary']), set(apps))

    def test_the_release_draws_only_the_screens_its_release_knows(self):
        screens = [('gauntlet', 16), ('wall', 20)]
        with tempfile.TemporaryDirectory() as folder:
            known = Path(folder) / 'screens.txt'
            with patch.object(desktop, 'RELEASE_SCREENS', known):
                _, report = self.rounds([24.3, 30.0], apps=['cranpose-release'], screens=screens)
                self.assertEqual(report['scenarios'][1]['legs'], [], 'a release that wrote no screens has the gauntlet alone')
                known.write_text('gauntlet\nwall\n')
                _, report = self.rounds([24.3, 30.0, 20.0, 22.0], apps=['cranpose-release'], screens=screens)
                self.assertEqual(len(report['scenarios'][1]['legs']), 2)

    def test_each_round_clears_the_apps_state_before_the_first_start_and_keeps_it_for_the_second(self):
        measured, scenario = self.rounds([24.3, 30.0, 24.6, 30.5], rounds=2)
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-second-1', 'fyne-2-first-1', 'fyne-2-second-1'])
        self.assertEqual(self.cleared, [(0, 'chrome-profile-fyne-1'), (2, 'chrome-profile-fyne-2')],
                         "each round's first start begins on cleared state and a profile of its own")
        self.assertEqual([leg['start'] for leg in scenario['legs']], ['first', 'second'] * 2)
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 24.45)
        self.assertEqual(scenario['starts']['second']['summary']['fyne']['fps'], 30.25)

    def test_each_first_start_runs_a_fresh_install_and_the_second_start_runs_the_same(self):
        _, scenario = self.rounds([24.3, 30.0, 24.6, 30.5], rounds=2)
        self.assertEqual(self.programs, ['wait', 'install fyne-1-first-1', 'installs/fyne-1-first-1/fyne',
                                         'installs/fyne-1-first-1/fyne',
                                         'wait', 'install fyne-2-first-1', 'installs/fyne-2-first-1/fyne',
                                         'installs/fyne-2-first-1/fyne'],
                         'macOS checks a program the first time it runs, as after an install, once the '
                         'checks queued before it are done')
        self.assertEqual([leg.get('program_checks_waited_s') for leg in scenario['legs']], [2.5, None] * 2)

    def test_a_leg_far_off_the_earlier_ones_of_its_start_is_measured_once_more_and_the_median_decides(self):
        measured, scenario = self.rounds([24.3, 30.0, 60.1, 24.6, 30.5], rounds=2)
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-second-1', 'fyne-2-first-1',
                                    'fyne-2-first-again-1', 'fyne-2-second-1'])
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 24.6)
        self.assertEqual(self.cleared, [(0, 'chrome-profile-fyne-1'), (2, 'chrome-profile-fyne-2'),
                                        (3, 'chrome-profile-fyne-2')],
                         'a first start measured again begins on cleared state again')

    def test_a_failed_attempt_is_measured_again(self):
        measured, scenario = self.rounds([RuntimeError('FrameCount: process 1 shows no window'), 24.3, 30.0])
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-first-2', 'fyne-1-second-1'])
        self.assertEqual([leg['fps'] for leg in scenario['legs']], [24.3, 30.0])

    def test_an_app_no_first_start_measured_leaves_its_legs_out_and_the_run_goes_on(self):
        failure = RuntimeError('fyne.log: no "PERF first_frame" within 60 s')
        measured, scenario = self.rounds([failure, failure, failure])
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-first-2', 'fyne-1-first-3'],
                         'no second start without a first one')
        self.assertEqual(scenario['legs'], [])
        self.assertEqual(scenario['starts']['first']['summary']['fyne'], {}, 'no values: the dashboard shows a dash')
        self.assertEqual([(failure['subject'], failure['start']) for failure in scenario['failures']],
                         [('fyne', 'first')] * 3,
                         'each failed attempt is kept, so the dashboard can say why fyne has no frames')

    def test_a_browser_run_is_a_run_of_its_own_kind_on_a_device_named_for_the_browser(self):
        with patch.object(desktop.versions, 'chrome_version', return_value='154.0.8037.98'):
            _, report = self.rounds([24.3, 30.0], browser=Path('dist'), app='web')
        self.assertEqual(report['kind'], 'browser')
        self.assertEqual(report['device']['ro.product.model'], 'Apple M3 Pro · Chrome 154')
        self.assertEqual(report['scenarios'][0]['extras'], 'tier 16, 1280 x 820 browser window')
        self.assertEqual(report['subjects'][0]['label'], 'Web Chrome 154.0.8037.98')
        self.assertEqual(report['protocol']['starts'], ['first', 'second'])

    def test_no_leg_of_a_browser_run_is_chromes_first_launch_of_the_run(self):
        with patch.object(desktop.versions, 'chrome_version', return_value='154.0.8037.98'):
            self.rounds([24.3, 30.0, 24.6, 30.5], rounds=2, browser=Path('dist'), app='web')
        self.assertEqual(self.warmed, [0], 'Chrome opens once before the first leg, and only then')
        self.rounds([24.3, 30.0])
        self.assertEqual(self.warmed, [], 'a native run opens no Chrome of its own')

    def test_a_disturbed_leg_stays_in_the_run_marked_and_out_of_the_medians(self):
        measured, scenario = self.rounds([(10.0, 3.0), 24.3, 30.0])
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-first-2', 'fyne-1-second-1'])
        self.assertEqual([(leg['fps'], leg['disturbed']) for leg in scenario['legs']],
                         [(10.0, True), (24.3, False), (30.0, False)])
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 24.3)

    def test_a_machine_within_the_idle_ceiling_disturbs_only_the_legs_that_add_more_than_the_limit(self):
        # Within the 0.9 idle ceiling: at 0.8 idle cores, 2.2 cores of others
        # are 1.4 above it (within 1.5), and 3.0 cores are 2.2 above it.
        measured, scenario = self.rounds([(24.3, 2.2), (10.0, 3.0), 30.0], idle=0.8)
        self.assertEqual(
            [(leg['fps'], leg['disturbed'], leg['idle_other_cores'], leg['idle_waited_s']) for leg in scenario['legs']],
            [(24.3, False, 0.8, 0.0), (10.0, True, 0.8, 0.0), (30.0, False, 0.8, 0.0)],
        )
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 24.3)

    def test_a_busy_idle_reading_waits_for_a_quiet_host_and_records_the_wait(self):
        clock = [100.0]
        readings = iter([1.93, 1.24, 0.44])

        def fake_idle(span_s=desktop.IDLE_SPAN_S):
            clock[0] += span_s
            return next(readings)

        with patch.object(desktop.time, 'monotonic', side_effect=lambda: clock[0]):
            _, scenario = self.rounds([(56.2, 0.60), (56.4, 0.55)], idle=fake_idle)
        self.assertEqual(self.idle_calls, 3)
        self.assertEqual(
            [(leg['fps'], leg['disturbed'], leg['idle_other_cores'], leg['idle_waited_s']) for leg in scenario['legs']],
            [(56.2, False, 0.44, 10.0), (56.4, False, 0.44, 10.0)],
        )
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 56.2)

    def test_a_host_that_stays_busy_never_raises_the_disturbance_bar(self):
        # 2026-11-27 browser gauntlet: idle stayed at 1.93 cores while second
        # start ran at 49.1 fps with other_cores 2.42. A busy baseline must
        # never add to --max-others (1.5).
        clock = [100.0]

        def busy_idle(span_s=desktop.IDLE_SPAN_S):
            clock[0] += span_s
            return 1.93

        with patch.object(desktop.time, 'monotonic', side_effect=lambda: clock[0]):
            measured, scenario = self.rounds(
                [(54.1, 1.13), (49.1, 2.42), (49.5, 2.42), (49.3, 2.42)],
                idle=busy_idle,
                max_idle_wait=15.0,
            )
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-second-1', 'fyne-1-second-2', 'fyne-1-second-3'])
        self.assertEqual(
            [(leg['start'], leg['disturbed'], leg['idle_other_cores'], leg['idle_waited_s']) for leg in scenario['legs']],
            [
                ('first', False, 1.93, 15.0),
                ('second', True, 1.93, 15.0),
                ('second', True, 1.93, 15.0),
                ('second', True, 1.93, 15.0),
            ],
        )
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 54.1)
        self.assertEqual(scenario['starts']['second']['summary']['fyne'], {})

    def test_the_exclusive_host_lock_blocks_ci_during_a_subject_and_releases_between_subjects(self):
        with tempfile.TemporaryDirectory(prefix='cranpose-desktop-lock-') as folder:
            root = Path(folder)
            entered = root / 'shared-entered'
            env = host_lock_env(root)
            child_holder = []
            seen_by_subject = {}

            def measure(name, args, work, page, stage, label, earlier, profile, program=None):
                if name == 'egui' and not child_holder:
                    child, queued = queue_on_host_lock('--shared', entered, env)
                    child_holder.append(child)
                    self.assertTrue(queued, 'shared CI took the host lock during egui')
                    seen_by_subject['during_egui'] = entered.exists()
                elif name == 'fyne' and 'during_fyne' not in seen_by_subject:
                    seen_by_subject['during_fyne'] = entered.exists()
                return {'fps': 30.0, 'frames': 90, 'run_s': 3.0, 'first_frame_s': 0.4,
                        'frame_ms': [400.0, 416.7, 433.3], 'fps_by_second': [15.0, 30.0, 30.0],
                        'interval_p50_ms': 16.7, 'interval_p99_ms': 20.0, 'cpu_cores': 1.0,
                        'server_cores': 0.2, 'other_cores': 0.4, 'cpu_ms_per_frame': 40.0}

            args = SimpleNamespace(parity=False, output=root / 'results', rounds=1, max_others=1.5,
                                   max_idle=desktop.MAX_IDLE_OTHERS, max_idle_wait=15.0, host_lock=True,
                                   main=None, release=None, tier=16, run=3.0, browser=None,
                                   screens=[('gauntlet', 16)])
            try:
                with patch.dict(os.environ, env, clear=True), \
                        patch.object(desktop, 'measure', measure), \
                        patch.object(desktop, 'clear_app_state', lambda _: None), \
                        patch.object(desktop, 'app_bytes', return_value=2048), \
                        patch.object(desktop, 'fresh_install', lambda name, stage, label: name), \
                        patch.object(desktop, 'wait_for_program_checks', lambda stage: 0.0), \
                        patch.object(desktop, 'idle_others', return_value=0.4), \
                        patch.object(desktop, 'HOST_LOCK_YIELD_S', 0.5), \
                        patch.object(desktop.subprocess, 'run', return_value=SimpleNamespace(stdout='Apple M3 Pro')):
                    desktop.run(args, ['egui', 'fyne'], 'page', root / 'stage')
            finally:
                for child in child_holder:
                    output, _ = child.communicate(timeout=5)
                    self.assertEqual(child.returncode, 0, output)
            self.assertFalse(seen_by_subject['during_egui'], 'shared CI overlapped egui')
            self.assertTrue(seen_by_subject['during_fyne'], 'shared CI did not pass between egui and fyne')

    def test_every_frame_time_from_the_launch_is_kept_in_the_run(self):
        _, scenario = self.rounds([24.0, 26.0])
        self.assertEqual([leg['frame_ms'] for leg in scenario['legs']], [[400.0, 416.7, 433.3]] * 2)


class FrameCountBuildTest(unittest.TestCase):
    def test_a_framecount_build_keeps_a_concurrent_measurement_waiting(self):
        with tempfile.TemporaryDirectory(prefix='cranpose-framecount-lock-') as folder:
            root = Path(folder)
            entered = root / 'measurement-entered'
            env = host_lock_env(root)
            builds, measurements = [], []

            def build(command, **_):
                builds.append(command[1])
                child, queued = queue_on_host_lock('--exclusive', entered, env)
                measurements.append(child)
                self.assertTrue(queued, 'a measurement took the host lock during the FrameCount build')
                self.assertFalse(entered.exists(), 'a measurement ran during the FrameCount build')

            arguments = ['desktop.py', '--host-lock', '--output', str(root / 'out'), '--apps', 'fyne']
            report = {'kind': 'frameworks', 'scenarios': [{'starts': {}}]}
            try:
                with patch.dict(os.environ, env, clear=True), patch.object(sys, 'argv', arguments), \
                        patch.object(desktop, 'FRAMECOUNT', root / 'FrameCount.app'), \
                        patch.object(desktop.subprocess, 'run', build), \
                        patch.object(desktop, 'serve_page', return_value='page'), \
                        patch.object(desktop, 'run', return_value=report), \
                        patch.object(desktop, 'observe_display', return_value=({}, [])):
                    desktop.main()
            finally:
                for child in measurements:
                    output, _ = child.communicate(timeout=5)
                    self.assertEqual(child.returncode, 0, output)
            self.assertEqual(builds, [str(desktop.HERE / 'framecount/build.sh')])
            self.assertTrue(entered.exists(), 'the measurement ran once the build was done')


class FirstFrameTest(unittest.TestCase):
    """`frame_stats` on what FrameCount reports of a window that shows its
    background before the app's first frame, as Flutter's, Slint's and
    Dioxus's do on macm3."""

    LAUNCHED = 100.0
    # Two pictures of the background 230 ms apart, then the app's frames
    # 1/60 s apart.
    SHOWN = [0.30, 0.53] + [0.76 + n / 60 for n in range(6)]

    def frames(self, content):
        shown = {} if content is None else {'first_content': self.LAUNCHED + content}
        return {'times': [self.LAUNCHED + time for time in self.SHOWN], 'capture_start': self.LAUNCHED + 0.25,
                'composited': len(self.SHOWN), 'link': {'hz': 60.0, 'max_gap_ms': 17.0, 'nominal_hz': 60.0},
                'display': {'id': 1, 'active': True, 'refresh_hz': 60.0, 'scale': 2.0}, **shown}

    def test_a_window_counts_from_its_first_picture_with_content(self):
        _, leg = desktop.frame_stats('flutter', self.frames(0.76), self.LAUNCHED, 3.0, None)
        self.assertEqual(leg['first_frame_s'], 0.76)
        self.assertEqual(leg['interval_p99_ms'], 16.67)
        # Every picture of the run counts toward its rate and its seconds.
        self.assertEqual(leg['frames'], 8)
        self.assertEqual(leg['fps_by_second'], [8, 0, 0])

    def test_a_page_whose_own_parts_show_first_counts_from_its_first_frame_line(self):
        _, leg = desktop.frame_stats('web', self.frames(0.53), self.LAUNCHED, 3.0, self.LAUNCHED + 0.77)
        self.assertEqual(leg['first_frame_s'], 0.777)

    def test_a_window_that_shows_nothing_but_its_background_fails_the_leg(self):
        with self.assertRaisesRegex(RuntimeError, 'no content shown'):
            desktop.frame_stats('slint', self.frames(None), self.LAUNCHED, 3.0, None)


class AppStateTest(unittest.TestCase):
    def test_clearing_removes_the_apps_caches_and_the_profile_and_nothing_else(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for path in ['cache/com.apple.metal/16777235_419/functions.data',
                         'cache/dev.perfcompare.swiftui/com.apple.metal/x', 'cache/dev.other/kept',
                         'support/perf-compare/pipeline-cache.bin', 'support/Code/kept',
                         'profile/Default/GPUCache/data_0']:
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text('x')
            state = [(root / 'cache', 'com.apple.metal'), (root / 'cache', 'dev.perfcompare.*'),
                     (root / 'support', 'perf-compare*')]
            with patch.object(desktop, 'APP_STATE', state):
                desktop.clear_app_state(root / 'profile')
            left = sorted(str(path.relative_to(root)) for path in root.rglob('*') if path.is_file())
            self.assertEqual(left, ['cache/dev.other/kept', 'support/Code/kept'])


@unittest.skipUnless(sys.platform == 'darwin', 'macOS signs programs with codesign')
class FreshInstallTest(unittest.TestCase):
    @staticmethod
    def signature(path, field):
        details = subprocess.run(['codesign', '-dv', '--verbose=3', str(path)], capture_output=True, text=True,
                                 check=True).stderr
        return next(line for line in details.splitlines() if line.startswith(f'{field}='))

    def test_every_install_runs_its_program_under_its_name_with_a_code_hash_no_earlier_install_had(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'main.c').write_text('int main(void) { return 7; }\n')
            built = root / 'perf-compare-stub'
            subprocess.run(['cc', '-o', str(built), str(root / 'main.c')], check=True)
            with patch.dict(desktop.APPS, {'stub': [built]}):
                installs = [desktop.fresh_install('stub', root, label) for label in ('a', 'b')]
            programs = [built, *installs]
            self.assertEqual(len({self.signature(path, 'CDHash') for path in programs}), 3)
            self.assertEqual(len({self.signature(path, 'Identifier') for path in programs}), 1)
            for path in installs:
                self.assertEqual(subprocess.run([path]).returncode, 7)

    def test_every_install_gives_the_libraries_beside_its_program_new_code_hashes(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            built = root / 'app'
            built.mkdir()
            (root / 'seven.c').write_text('int seven(void) { return 7; }\n')
            (root / 'main.c').write_text('int seven(void);\nint main(void) { return seven(); }\n')
            subprocess.run(['cc', '-dynamiclib', '-install_name', '@executable_path/libseven.dylib',
                            '-o', str(built / 'libseven.dylib'), str(root / 'seven.c')], check=True)
            subprocess.run(['cc', '-o', str(built / 'main'), str(root / 'main.c'), str(built / 'libseven.dylib')],
                           check=True)
            with patch.dict(desktop.APPS, {'stub': [built / 'main']}), \
                    patch.object(desktop, 'INSTALLED_WITH_DIRECTORY', {'stub'}):
                installs = [Path(desktop.fresh_install('stub', root, label)) for label in ('a', 'b')]
            libraries = [built / 'libseven.dylib', *(path.parent / 'libseven.dylib' for path in installs)]
            self.assertEqual(len({self.signature(path, 'CDHash') for path in libraries}), 3)
            for path in installs:
                self.assertEqual(subprocess.run([path]).returncode, 7)

    def test_a_first_start_waits_while_new_programs_start_slower_than_a_checked_one(self):
        with tempfile.TemporaryDirectory() as folder:
            stage = Path(folder)
            with patch.multiple(desktop, CHECKED_IN_S=0.0, CHECK_QUEUE_WAIT_S=1.5):
                waited = desktop.wait_for_program_checks(stage)
            self.assertGreaterEqual(waited, 1.5)
            self.assertGreater(len(list((stage / 'probes').glob('*/probe'))), 1)

    def test_a_first_start_goes_on_once_a_new_program_starts_as_a_checked_one(self):
        with tempfile.TemporaryDirectory() as folder:
            stage = Path(folder)
            with patch.multiple(desktop, CHECKED_IN_S=60.0, CHECK_QUEUE_WAIT_S=120.0):
                desktop.wait_for_program_checks(stage)
            self.assertEqual(len(list((stage / 'probes').glob('*/probe'))), 1)

    def test_a_failed_leg_quotes_the_panic_and_not_the_hint_after_it(self):
        log = ['Initialising in parallel using 10 threads',
               "thread 'main' panicked at wgpu_core.rs:3411:5:",
               'wgpu error: Validation Error',
               'Caused by:',
               'In Device::create_bind_group',
               'Buffer binding 1 range 201326592 exceeds `max_*_buffer_binding_size` limit 134217728',
               'note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace']
        words = desktop.last_words(log)
        self.assertIn('Buffer binding 1 range 201326592 exceeds', words)
        self.assertNotIn('RUST_BACKTRACE', words)

    def test_a_failed_leg_without_a_panic_quotes_its_last_line(self):
        self.assertEqual(desktop.last_words(['PERF first_frame']), ', last log line: PERF first_frame')
        self.assertEqual(desktop.last_words([]), ', logged nothing')


if __name__ == '__main__':
    unittest.main()
