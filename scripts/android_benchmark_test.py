import io
import json
import os
import signal
import time
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from unittest.mock import Mock, patch
from types import SimpleNamespace

import android_benchmark as benchmark
import android_benchmark_artifacts as artifacts
import android_benchmark_support as support
from android_benchmark_device import AndroidDevice, surface_frames, verify_route_window
from android_benchmark_build import build, native_artifact
from android_benchmark import run_leg, sequence, validate_pair, validate_route
from android_benchmark_video import AndroidRecording, ScrcpyRecording, inspect_recording


class BenchmarkContracts(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.report = self.root / 'report.json'

    def source(self):
        source = self.root / 'source'
        source.mkdir()
        (source / 'app.rs').write_text('fn main() {}')
        (source / 'run.sh').write_text('exit 0\n')
        (source / 'run.sh').chmod(0o755)
        return source

    def test_archives_are_deterministic_complete_and_extract_into_fresh_sources(self):
        source = self.source()
        archives = [self.root / f'{index}.tar.gz' for index in range(2)]
        hashes = [artifacts.snapshot_source(source, path) for path in archives]
        self.assertEqual(hashes[0], hashes[1])
        extracted = self.root / 'extracted'
        inventory = artifacts.extract_source(archives[0], extracted)
        self.assertEqual(inventory, artifacts.source_inventory(source))
        self.assertTrue(os.access(extracted / 'run.sh', os.X_OK))
        (extracted / 'unexpected.rs').write_text('stale input')
        with self.assertRaisesRegex(ValueError, 'fresh directory'):
            artifacts.extract_source(archives[0], extracted)

    def test_archive_rejects_symlinks_and_self_inclusion(self):
        source = self.source()
        with self.assertRaisesRegex(ValueError, 'outside'):
            artifacts.snapshot_source(source, source / 'self.tar.gz')
        (source / 'alias.rs').symlink_to(source / 'app.rs')
        with self.assertRaisesRegex(ValueError, 'symlink'):
            artifacts.snapshot_source(source, self.root / 'link.tar.gz')

    def test_archive_rejects_paths_duplicates_and_corrupt_inventory_before_extraction(self):
        valid = self.root / 'valid.tar.gz'
        artifacts.snapshot_source(self.source(), valid)
        for index, mutation in enumerate(['../escape', '/absolute', 'duplicate', 'corrupt', 'extra']):
            archive = self.root / f'bad-{index}.tar'
            with tarfile.open(valid) as original, tarfile.open(archive, 'w') as modified:
                for member in original.getmembers():
                    data = original.extractfile(member).read()
                    if mutation == 'corrupt' and member.name == 'app.rs':
                        data = b'changed bytes'
                    member.size = len(data)
                    modified.addfile(member, io.BytesIO(data))
                if mutation != 'corrupt':
                    extra = tarfile.TarInfo('app.rs' if mutation == 'duplicate' else mutation)
                    extra.size = 1
                    modified.addfile(extra, io.BytesIO(b'x'))
            destination = self.root / f'rejected-{index}'
            with self.assertRaises(ValueError):
                artifacts.extract_source(archive, destination)
            self.assertFalse(destination.exists())
        self.assertFalse((self.root / 'escape').exists())

    def test_work_and_cleanup_failures_are_retained_and_every_cleanup_runs(self):
        report, calls = {}, []

        def work():
            raise subprocess.CalledProcessError(3, ['broken-input'], output=b'injection refused')

        def cleanup():
            self.assertEqual(json.loads(self.report.read_text())['status'], 'running')
            raise RuntimeError('restore failed')

        with self.assertRaises(BaseExceptionGroup):
            support.run_reported(self.report, report, work, [cleanup, lambda: calls.append('last')])
        saved = json.loads(self.report.read_text())
        self.assertEqual(calls, ['last'])
        self.assertEqual(saved['status'], 'failed')
        self.assertFalse(saved['acceptance_eligible'])
        self.assertEqual(saved['errors'][0]['output'], 'injection refused')
        self.assertIn('restore failed', saved['cleanup_errors'][0])

    def test_reporting_failure_does_not_skip_device_cleanup(self):
        calls = []
        original = support.write_report
        writes = 0

        def fail_second_write(path, report):
            nonlocal writes
            writes += 1
            if writes == 2:
                raise OSError('disk full')
            original(path, report)

        with patch.object(support, 'write_report', side_effect=fail_second_write):
            with self.assertRaises(BaseException):
                support.run_reported(self.report, {}, lambda: None, [lambda: calls.append('restored')])
        self.assertEqual(calls, ['restored'])
        self.assertEqual(json.loads(self.report.read_text())['status'], 'failed')

    def test_nested_command_failures_keep_their_diagnostics(self):
        def work():
            raise ExceptionGroup('recording', [subprocess.CalledProcessError(
                2, ['ffprobe', 'video.mp4'], output=b'invalid container')])
        with self.assertRaises(BaseExceptionGroup):
            support.run_reported(self.report, {}, work)
        failure = json.loads(self.report.read_text())['errors'][0]['children'][0]
        self.assertEqual(failure['command'], ['ffprobe', 'video.mp4'])
        self.assertEqual(failure['output'], 'invalid container')

    def test_success_does_not_implicitly_authorize_a_performance_number(self):
        report = {}
        support.run_reported(self.report, report, lambda: report.update(fps=60))
        self.assertEqual(report['status'], 'complete')
        self.assertFalse(report['acceptance_eligible'])

    def test_performance_is_not_eligible_until_cleanup_succeeds(self):
        report = {}
        def cleanup():
            saved = json.loads(self.report.read_text())
            self.assertEqual(saved['status'], 'running')
            self.assertFalse(saved['acceptance_eligible'])
        support.run_reported(self.report, report, lambda: report.update(acceptance_eligible=True), [cleanup])
        self.assertTrue(report['acceptance_eligible'])

    def test_timeout_preserves_command_output_and_stops_the_child(self):
        log = self.root / 'command.log'
        with self.assertRaises(subprocess.TimeoutExpired):
            support.checked_command([sys.executable, '-c',
                                     'import os,time; print(os.getpid(), flush=True); time.sleep(60)'],
                                    timeout=1, output=log)
        pid = int(log.read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_sigterm_interrupts_pending_commands_and_retains_failure_report(self):
        marker = self.root / 'command.pid'
        sleeper = f'import os,time; from pathlib import Path; Path({str(marker)!r}).write_text(str(os.getpid())); time.sleep(60)'
        driver = ('import signal,sys; import android_benchmark as b; import android_benchmark_support as s; '
                  'signal.signal(signal.SIGTERM,b.interrupted); '
                  f's.run_reported({str(self.report)!r}, {{}}, lambda: s.checked_command([sys.executable,"-c",{sleeper!r}],timeout=60))')
        with (self.root / 'driver.log').open('wb') as log:
            process = subprocess.Popen([sys.executable, '-c', driver], cwd=Path(__file__).parent,
                                       stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                deadline = time.monotonic() + 5
                while not marker.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertTrue(marker.exists(), 'Child command did not start')
                process.send_signal(signal.SIGTERM)
                self.assertNotEqual(process.wait(timeout=3), 0)
                result = json.loads(self.report.read_text())
                self.assertEqual(result['status'], 'failed')
                self.assertFalse(result['acceptance_eligible'])
                with self.assertRaises(ProcessLookupError):
                    os.kill(int(marker.read_text()), 0)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                if marker.exists():
                    try:
                        os.kill(int(marker.read_text()), signal.SIGTERM)
                    except ProcessLookupError:
                        pass

    def test_device_lock_is_shared_between_processes_and_released_after_failure(self):
        serial = 'fixture-' + str(self.root)
        marker = self.root / 'acquired'
        code = ('from android_robot_device import locked_device; from pathlib import Path; '
                'import sys\nwith locked_device(sys.argv[1]): Path(sys.argv[2]).touch()')
        with support.device_lock(serial):
            child = subprocess.Popen([sys.executable, '-c', code, serial, str(marker)],
                                     cwd=Path(__file__).parent)
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    child.wait(timeout=0.2)
                self.assertFalse(marker.exists())
            except BaseException:
                child.terminate()
                child.wait(timeout=5)
                raise
        self.assertEqual(child.wait(timeout=5), 0)
        self.assertTrue(marker.exists())

    def test_partial_property_setup_is_restored_and_conflicts_are_not_overwritten(self):
        class Device(AndroidDevice):
            def __init__(self):
                super().__init__('test')
                self.values = {'debug.cranpose.a': 'original', 'debug.cranpose.b': 'saved'}
                self.fail = True

            def properties(self):
                return self.values.copy()

            def shell(self, *parts):
                _, name, value = parts
                if self.fail and name.endswith('.b'):
                    self.fail = False
                    raise RuntimeError('property write refused')
                self.values[name] = value

        device = Device()
        original = device.values.copy()
        with self.assertRaisesRegex(RuntimeError, 'refused'):
            device.configure_properties({})
        device.restore_properties()
        self.assertEqual(device.values, original)
        device.configure_properties({'debug.cranpose.new': '1'})
        device.values['debug.cranpose.a'] = 'external change'
        with self.assertRaises(ExceptionGroup):
            device.restore_properties()
        self.assertEqual(device.values['debug.cranpose.a'], 'external change')
        self.assertEqual(device.values['debug.cranpose.b'], 'saved')
        self.assertEqual(device.values['debug.cranpose.new'], '')

    def test_status_bar_changes_do_not_prove_application_motion(self):
        from PIL import Image
        device = AndroidDevice('test')
        device.wake = lambda: None
        captures = []
        image = Image.new('RGB', (100, 100), 'white')
        for index, point in enumerate([(0, 0), (1, 0), (50, 50)]):
            image.putpixel(point, (0, 0, 0))
            encoded = io.BytesIO()
            image.save(encoded, format='PNG')
            device.run = lambda *parts: encoded.getvalue()
            captures.append(device.screenshot(self.root / f'{index}.png', [10, 10, 90, 90]))
        self.assertNotEqual(captures[0]['sha256'], captures[1]['sha256'])
        self.assertEqual(captures[0]['motion_pixels_sha256'], captures[1]['motion_pixels_sha256'])
        self.assertNotEqual(captures[1]['motion_pixels_sha256'], captures[2]['motion_pixels_sha256'])

    def test_surface_counts_reject_missing_or_restarted_application_layers(self):
        valid = 'layerName = SurfaceView - com.example.app/Main\ntotalFrames = 300\n'
        self.assertEqual(surface_frames(valid, 'com.example.app')['frames'], 300)
        for text in ['', valid + valid, valid.replace('300', '0'), valid.replace('example.app', 'other')]:
            with self.assertRaises(ValueError):
                surface_frames(text, 'com.example.app')

    def test_route_accepts_completed_motion_and_rejects_dropped_input_and_overruns(self):
        valid = ('gesture=0 start=0 end=1500\ngesture=1 start=1666 end=3166\n'
                 'elapsed_ms=3332\nSF_BEGIN\nframes\nSF_END\n')
        self.assertEqual(verify_route_window(valid, 2, 1500, 1666, 3332, 30), (3.332, 'frames\n'))
        for text in [valid.replace('gesture=1', 'dropped=1'), valid.replace('3332', '3999'),
                     valid.replace('end=1500', 'end=30'), valid.replace('start=1666', 'start=2000'),
                     valid.replace('SF_END', 'missing')]:
            with self.assertRaises(ValueError):
                verify_route_window(text, 2, 1500, 1666, 3332, 30)

    def test_native_artifact_comes_from_the_selected_cargo_package(self):
        exported = self.root / 'native/arm64-v8a'
        exported.mkdir(parents=True)
        library = exported / 'libapp.so'
        library.write_bytes(b'compiled library')
        (exported / 'libdependency.so').write_bytes(b'dependency')
        package = {'targets': [{'name': 'app', 'crate_types': ['cdylib']},
                               {'name': 'build-script-build', 'crate_types': ['bin']}]}
        self.assertEqual(native_artifact(exported.parent, package, 'arm64-v8a', b''), library)
        with self.assertRaisesRegex(ValueError, 'did not export'):
            native_artifact(exported.parent, package, 'armeabi-v7a', b'')
        with self.assertRaisesRegex(ValueError, 'warnings'):
            native_artifact(exported.parent, package, 'arm64-v8a', b'warning: build warning\n')

    def test_build_resolves_archived_framework_before_reading_unavailable_host_paths(self):
        framework = self.root / 'framework'
        package = framework / 'crates/benchmark-fixture'
        package.mkdir(parents=True)
        (package / 'Cargo.toml').write_text('[package]\nname="benchmark-fixture"\nversion="1.0.0"\nedition="2021"\n[lib]\npath="lib.rs"\n')
        (package / 'lib.rs').write_text('pub fn value() -> u32 { 1 }\n')
        app = self.root / 'app'
        (app / '.cargo').mkdir(parents=True)
        (app / 'Cargo.toml').write_text('[package]\nname="benchmark-app"\nversion="1.0.0"\nedition="2021"\n[lib]\npath="lib.rs"\ncrate-type=["cdylib"]\n[dependencies]\nbenchmark-fixture="1.0.0"\n')
        (app / 'lib.rs').write_text('pub fn value() -> u32 { benchmark_fixture::value() }\n')
        config = app / '.cargo/config.toml'
        config.write_text('[patch.crates-io]\nbenchmark-fixture={path=' + json.dumps(str(package)) + '}\n')
        support.checked_command(['cargo', 'generate-lockfile', '--offline'], cwd=app)
        config.write_text('[patch.crates-io]\nbenchmark-fixture={path=' + json.dumps(str(self.root / 'unavailable-host')) + '}\n')
        output = self.root / 'build'
        output.mkdir()
        ndk = self.root / 'ndk'
        ndk.mkdir()
        (ndk / 'source.properties').write_text('Pkg.Revision=27.0.12077973\n')
        args = SimpleNamespace(framework=framework, app=app, cache=self.root / 'cache',
                               target_dir=self.root / 'target', output=output, features='',
                               package='benchmark-app', platform=24, abi='arm64-v8a', timeout=30)
        report = {}

        def checked(command, **options):
            if command[:2] == ['cargo', 'ndk']:
                raise RuntimeError('native build reached with archived sources')
            try:
                return support.checked_command(command, **options)
            except subprocess.CalledProcessError as error:
                sys.stderr.write(error.output.decode())
                raise

        with patch.dict(os.environ, {'ANDROID_NDK_HOME': str(ndk)}), patch('android_benchmark_build.checked_command', side_effect=checked):
            with self.assertRaisesRegex(RuntimeError, 'native build reached'):
                build(args, report)
        self.assertTrue(Path(report['resolved']['benchmark-fixture']).is_relative_to(Path(report['cache']) / 'framework'))
        self.assertIn('unavailable-host', config.read_text())

    def test_endpoint_reads_the_dump_file_and_rejects_offscreen_or_other_apps(self):
        device = AndroidDevice('fixture')
        calls = []
        document = ('<?xml version="1.0"?><hierarchy><node package="com.scene" '
                    'text="Last row" bounds="[10,20][90,80]"/></hierarchy>')

        def shell(*parts):
            calls.append(parts)
            if parts[0] == 'cat':
                return document
            if parts[0] == 'uiautomator':
                return 'UI hierchary dumped to: ' + parts[-1]
            return ''

        with patch.object(device, 'wake'), patch.object(device, 'shell', side_effect=shell):
            self.assertEqual(device.endpoint('com.scene', 'Last row', [100, 100])[0]['bounds'], [10, 20, 90, 80])
            for invalid in ['package="other.app"', 'bounds="[0,150][100,180]"']:
                document = document.replace('package="com.scene"', invalid) if invalid.startswith('package') else (
                    document.replace('package="other.app"', 'package="com.scene"').replace('bounds="[10,20][90,80]"', invalid))
                with self.assertRaisesRegex(ValueError, 'endpoint'):
                    device.endpoint('com.scene', 'Last row', [100, 100])
        dumps = [parts[-1] for parts in calls if parts[0] == 'uiautomator']
        self.assertEqual(len(set(dumps)), 3)
        self.assertEqual([parts[-1] for parts in calls if parts[0] == 'rm'], dumps)

    def test_device_cleanup_waits_for_its_process_and_never_signals_a_reused_pid(self):
        device = AndroidDevice('fixture')
        calls = []
        running = 'route-unique-token'

        def shell(*parts):
            nonlocal running
            calls.append(parts)
            if parts[0] == 'kill':
                running = 'different-owner'
                return ''
            return running

        with patch.object(device, 'shell', side_effect=shell):
            device.stop_owned_process('123', 'route-unique-token')
            device.stop_owned_process('123', 'route-unique-token')
        self.assertEqual([parts for parts in calls if parts[0] == 'kill'], [('kill', '-TERM', '123')])
        with patch.object(device, 'shell', return_value='route-unique-token'), patch('android_benchmark_device.time.monotonic', side_effect=[0, 6, 6, 9]):
            with self.assertRaisesRegex(RuntimeError, 'did not terminate'):
                device.stop_owned_process('123', 'route-unique-token')
        for pid in ['0', '1', '-123', '123; echo wrong']:
            with self.assertRaises(ValueError):
                device.stop_owned_process(pid, 'route-unique-token')

    def test_pair_rejects_different_build_settings_and_app_sources(self):
        build = dict.fromkeys(['abi', 'features', 'toolchain', 'cargo', 'ndk', 'settings', 'lock_sha256'], 'same')
        build['sources'] = {'app': {'inventory': {'scene': 'same'}}}
        first = {'build': build, 'payload': {'assets': 'same'}}
        validate_pair([first, first])
        for key in build:
            second = json.loads(json.dumps(first))
            if key == 'sources':
                second['build'][key]['app']['inventory']['scene'] = 'different'
            else:
                second['build'][key] = 'different'
            with self.assertRaises(ValueError):
                validate_pair([first, second])
        with self.assertRaisesRegex(ValueError, 'payload'):
            validate_pair([first, first | {'payload': {}}])

    def test_app_comparison_requires_identical_framework_sources(self):
        build = dict.fromkeys(['abi', 'features', 'toolchain', 'cargo', 'ndk', 'settings', 'lock_sha256'], 'same')
        build['sources'] = {name: {'inventory': {'source': 'same'}} for name in ['app', 'framework']}
        first = {'build': build, 'payload': {'assets': 'same'}}
        second = json.loads(json.dumps(first))
        second['build']['sources']['app']['inventory']['source'] = 'changed app'
        validate_pair([first, second], variant_source='app')
        with self.assertRaises(ValueError):
            validate_pair([first, second])
        second['build']['sources']['framework']['inventory']['source'] = 'changed framework'
        with self.assertRaisesRegex(ValueError, 'framework sources'):
            validate_pair([first, second], variant_source='app')

    def test_roundtrip_uses_independent_start_checks_and_accumulates_only_motion_windows(self):
        route = {'kind': 'scroll', 'package': 'com.scene', 'activity': 'Activity', 'size': [100, 100],
                 'density': 160, 'motion_region': [10, 10, 80, 80], 'x': 20, 'y_start': 70, 'y_end': 20,
                 'count': 2, 'duration_ms': 10, 'period_ms': 20, 'window_ms': 40, 'timing_tolerance_ms': 10,
                 'start_text': 'Heading', 'end_text': 'Footer', 'start_region': [0, 0, 100, 50],
                 'setup': [{'visible_text': 'Navigation', 'tap': [90, 90]}],
                 'return': {'y_start': 20, 'y_end': 70}}
        device = Mock(spec=AndroidDevice)
        device.run.return_value = b'Success'
        device.wait_for_pid.return_value = '123'
        device.installed_apk.return_value = {'path': '/installed.apk', 'sha256': 'hash'}
        device.screenshot.return_value = {'size': [100, 100]}
        device.shell.side_effect = lambda *parts, **_options: {
            'pm': 'package:/installed.apk' if parts[1] == 'path' else 'Success', 'sha256sum': 'hash /installed.apk',
            'pidof': '123', 'date': 'launch-time', 'wm': 'Physical density: 160',
        }.get(parts[0], '')
        measured = []

        def window(_device, selected, _dex, _directory, report):
            measured.append(selected)
            self.assertEqual(report['launch_time'], 'launch-time')
            report.update(elapsed_s=0.04, layer={'frames': 2}, before={}, after={}, acceptance_eligible=True)

        proof = {'apk_sha256': 'hash', 'build': {'native_sha256': 'native'}, 'native_member': 'libapp.so'}
        report = {}
        with patch('android_benchmark.verify_apk'), patch('android_benchmark.wait_ready') as ready, patch('android_benchmark.run_window', side_effect=window), patch('android_benchmark.verify_first_gesture') as motion:
            run_leg(device, route, self.root / 'app.apk', proof, self.root / 'classes.dex', self.root, report, '/staged.apk')
        motion.assert_called_once()
        self.assertIsNone(ready.call_args_list[0].args[1]['start_region'])
        self.assertEqual(ready.call_args_list[1].args[1]['start_region'], route['start_region'])
        self.assertEqual([(r['start_text'], r['end_text']) for r in measured], [('Heading', 'Footer'), ('Footer', 'Heading')])
        self.assertEqual(report['fps'], 50)
        self.assertEqual(report['elapsed_s'], 0.08)
        self.assertEqual(len(report['windows']), 2)
        self.assertTrue(all(w['status'] == 'complete' for w in report['windows']))

    def test_endpoint_region_rejects_a_persistent_navigation_label(self):
        device = AndroidDevice('fixture')
        document = ('<?xml version="1.0"?><hierarchy><node package="com.scene" '
                    'text="Settings" bounds="[10,75][90,100]"/></hierarchy>')
        with patch.object(device, 'wake'), patch.object(device, 'shell', return_value=document):
            self.assertTrue(device.endpoint('com.scene', 'Settings', [100, 100]))
            with self.assertRaisesRegex(ValueError, 'endpoint'):
                device.endpoint('com.scene', 'Settings', [100, 100], [0, 0, 100, 60])

    def test_image_endpoint_checks_rendered_text_inside_the_requested_region(self):
        from PIL import Image
        image = Image.new('RGB', (100, 100), 'white')
        image.paste('blue', (0, 70, 100, 100))
        encoded = io.BytesIO()
        image.save(encoded, format='PNG')
        device = AndroidDevice('fixture', ocr=self.root / 'ocr', evidence=self.root / 'endpoints')
        crops = []

        def recognize(command):
            with Image.open(command[1]) as crop:
                crops.append((crop.size, crop.getpixel((0, 0))))
            return b'[{"text":"Settings","confidence":1.0}]'

        with patch.object(device, 'wake'), patch.object(device, 'run', return_value=encoded.getvalue()), patch('android_benchmark_device.checked_command', side_effect=recognize):
            proof = device.endpoint('com.scene', 'Settings', [100, 100], [0, 0, 100, 60], 'image')
            self.assertTrue(Path(proof['image']).is_file())
            with self.assertRaisesRegex(ValueError, 'image endpoint'):
                device.endpoint('com.scene', 'Library', [100, 100], [0, 0, 100, 60], 'image')
        self.assertEqual(crops, [((100, 60), (255, 255, 255))] * 2)

    def test_image_endpoint_validates_the_motion_capture_without_recapturing(self):
        from PIL import Image
        path = self.root / 'end.png'
        Image.new('RGB', (100, 100), 'white').save(path)
        device = AndroidDevice('fixture', ocr=self.root / 'ocr', evidence=self.root)
        with patch.object(device, 'run', side_effect=AssertionError('must not recapture')), patch('android_benchmark_device.checked_command', return_value=b'[{"text":"Settings"}]'):
            proof = device.endpoint('com.scene', 'Settings', [100, 100], [0, 0, 100, 60], 'image', image_path=path)
        self.assertEqual(proof['image'], str(path))
        self.assertEqual(proof['pixels']['sha256'], support.digest(path))

    def test_end_capture_retains_transient_overlays_and_validates_the_same_frame(self):
        route = {'kind': 'scroll', 'package': 'com.scene', 'end_text': 'Settings', 'size': [100, 100],
                 'motion_region': [0, 0, 100, 100], 'end_region': [0, 0, 100, 60],
                 'text_source': 'image', 'endpoint_timeout_s': 2}
        device = Mock(spec=AndroidDevice)
        device.screenshot.side_effect = [{'sha256': 'covered'}, {'sha256': 'clear'}]
        device.endpoint.side_effect = [ValueError('Visible image endpoint was not reached: Settings'), {'text': 'Settings'}]
        report = {}
        with patch('android_benchmark.time.sleep'):
            benchmark.capture_end(device, route, self.root, report)
        self.assertEqual(report['end_image']['sha256'], 'clear')
        self.assertEqual(len(report['endpoint_attempts']), 2)
        for capture, validation in zip(device.screenshot.call_args_list, device.endpoint.call_args_list):
            self.assertEqual(capture.args[0], validation.kwargs['image_path'])
        self.assertIn('error', report['endpoint_attempts'][0])
        device.screenshot.side_effect = None
        device.endpoint.side_effect = ValueError('endpoint absent')
        with self.assertRaisesRegex(ValueError, 'endpoint absent'):
            benchmark.capture_end(device, route | {'endpoint_timeout_s': 0}, self.root, {})
        for invalid in [-1, float('inf'), float('nan')]:
            with self.assertRaisesRegex(ValueError, 'finite and nonnegative'):
                benchmark.capture_end(device, route | {'endpoint_timeout_s': invalid}, self.root, {})

    def test_sequence_rejects_nonpositive_transfer_timeout(self):
        for timeout in [0, -1]:
            with self.assertRaisesRegex(ValueError, 'Transfer timeout'):
                sequence(SimpleNamespace(transfer_timeout_seconds=timeout), {})

    def test_sequence_stages_each_apk_once_and_cleans_up_success_and_failure(self):
        helper = self.root / 'helper'
        helper.mkdir()
        dex = helper / 'classes.dex'
        dex.write_bytes(b'route')
        (helper / 'route.json').write_text(json.dumps({
            'dex_sha256': support.digest(dex), 'source_sha256': support.digest(
                Path(__file__).parent / 'android/CranposeBenchmarkRoute.java')}))
        route = Path(__file__).parent / 'android/routes/huawei-showcase.json'
        proofs = []
        for name in ['a', 'b']:
            apk = self.root / f'{name}.apk'
            apk.write_bytes(name.encode())
            build = dict.fromkeys(['abi', 'features', 'toolchain', 'cargo', 'ndk', 'settings', 'lock_sha256'], 'same')
            build['sources'] = {'app': {'inventory': {}}}
            proof = self.root / f'{name}.json'
            proof.write_text(json.dumps({'status': 'complete', 'apk': apk.name,
                                         'apk_sha256': support.digest(apk), 'native_member': 'library',
                                         'build_directory': str(self.root), 'build': build, 'payload': {}}))
            proofs.append(proof)
        for fail in [False, True]:
            with self.subTest(fail=fail):
                output = self.root / str(fail)
                output.mkdir()
                args = SimpleNamespace(serial='fixture-' + str(self.root), adb='adb', route=route,
                                       dex=dex, ocr=None, record=False, video_bit_rate=2_000_000, a=proofs[0], b=proofs[1], output=output, transfer_timeout_seconds=600, variant_source='framework')
                device = Mock(spec=AndroidDevice)
                device.saved_properties = {}
                device.installed_apk.return_value = None
                uploaded, removed, slots = {}, [], []

                def push(*parts, **options):
                    self.assertEqual(device.wake.call_count, len(uploaded) + 1)
                    self.assertEqual(options['timeout'], 600)
                    self.assertEqual(parts[0], 'push')
                    self.assertNotIn(parts[2], uploaded)
                    uploaded[parts[2]] = support.digest(parts[1])
                    return b''

                def shell(*parts, **_options):
                    if parts[0] == 'sha256sum':
                        return uploaded[parts[1]] + ' ' + parts[1]
                    if parts[0] == 'rm':
                        removed.append(parts[-1])
                    return ''

                def leg(_device, _route, _apk, proof, _dex, _directory, report, remote, recorder):
                    self.assertIsNone(recorder)
                    self.assertEqual(uploaded[remote], proof['apk_sha256'])
                    slots.append(report['slot'])
                    if fail and len(slots) == 3:
                        raise RuntimeError('input refused')
                    report.update(fps=10 if report['slot'] == 'A' else 20,
                                  elapsed_s=1, acceptance_eligible=True)

                device.run.side_effect = push
                device.shell.side_effect = shell
                report = {}
                with patch('android_benchmark.AndroidDevice', return_value=device), patch('android_benchmark.verify_apk'), patch('android_benchmark.verify_build'), patch('android_benchmark.snapshot_source', return_value='hash'), patch('android_benchmark.source_inventory', return_value={}), patch('android_benchmark.run_leg', side_effect=leg), patch('builtins.print'):
                    if fail:
                        with self.assertRaises(BaseExceptionGroup):
                            sequence(args, report)
                    else:
                        sequence(args, report)
                self.assertEqual(''.join(slots), 'ABA' if fail else 'ABABBABA')
                self.assertEqual(len(uploaded), 2)
                self.assertEqual(set(removed), set(uploaded))
                device.restore_properties.assert_called_once()
                self.assertEqual(report['status'], 'failed' if fail else 'complete')
                self.assertEqual(report['acceptance_eligible'], not fail)
                if not fail:
                    self.assertEqual(report['mean_fps'], {'A': 10, 'B': 20})

    def test_recorded_windows_never_pass_fps_acceptance(self):
        route = json.loads((Path(__file__).parent / 'android/routes/huawei-showcase.json').read_text())
        dex = self.root / 'route.dex'
        dex.write_bytes(b'route')
        for recording in [False, True]:
            with self.subTest(recording=recording):
                device = Mock(spec=AndroidDevice)
                device.screenshot.return_value = {'motion_pixels_sha256': 'start'}
                device.state.return_value = {'temperature_c': 30}
                device.shell.side_effect = lambda *parts, **_options: support.digest(dex) if parts[0] == 'sha256sum' else 'route output'
                report = {'pid': '123', 'launch_time': 'start', 'recording': recording}
                def end(_device, _route, _directory, report):
                    report['end_image'] = {'motion_pixels_sha256': 'end'}
                stats = 'layerName = ' + route['package'] + '/Activity\ntotalFrames = 60'
                with patch('android_benchmark.wait_ready'), patch('android_benchmark.capture_end', side_effect=end), patch('android_benchmark.verify_route_window', return_value=(1.0, stats)):
                    benchmark.run_window(device, route, dex, self.root, report)
                self.assertEqual(report['fps'], 60)
                self.assertEqual(report['acceptance_eligible'], not recording)

    def test_install_timeout_accepts_only_the_expected_installed_apk(self):
        for installed in ['expected', 'different']:
            with self.subTest(installed=installed):
                device = Mock(spec=AndroidDevice)
                device.installed_apk.side_effect = [None, {'path': '/installed.apk', 'sha256': installed}]
                def shell(*parts, **_options):
                    if parts[:2] == ('pm', 'install'):
                        raise subprocess.TimeoutExpired(['pm', 'install'], 120, output=b'pending')
                    if parts[:2] == ('pm', 'path'):
                        return 'package:/installed.apk'
                    return installed + ' /installed.apk'
                device.shell.side_effect = shell
                report = {}
                if installed == 'expected':
                    benchmark.install_staged(device, 'com.scene', '/staged.apk', 'expected', report)
                    self.assertIn('timeout', report['installation'])
                else:
                    with self.assertRaisesRegex(ValueError, 'Installed APK'):
                        benchmark.install_staged(device, 'com.scene', '/staged.apk', 'expected', report)

    def test_matching_installed_apk_skips_installation_and_transfer(self):
        device = Mock(spec=AndroidDevice)
        device.installed_apk.return_value = {'path': '/installed.apk', 'sha256': 'expected'}
        report = {}
        benchmark.install_staged(device, 'com.scene', '/staged.apk', 'expected', report)
        device.shell.assert_not_called()
        device.shell.return_value = 'expected /staged.apk'
        benchmark.stage_apk(device, 'com.scene', self.root / 'app.apk', '/staged.apk', 'expected', 120)
        device.run.assert_not_called()
        device.shell.assert_any_call('cp', '/installed.apk', '/staged.apk')

    def test_installed_apk_rejects_split_packages_and_reads_exact_hash(self):
        device = AndroidDevice('fixture')
        with patch.object(device, 'shell', side_effect=['package:/base.apk', 'hash /base.apk']):
            self.assertEqual(device.installed_apk('com.scene'), {'path': '/base.apk', 'sha256': 'hash'})
        with patch.object(device, 'shell', return_value=''):
            self.assertIsNone(device.installed_apk('com.scene'))
        with patch.object(device, 'shell', return_value='package:/base.apk\npackage:/split.apk'):
            with self.assertRaisesRegex(ValueError, 'single'):
                device.installed_apk('com.scene')

    def test_launch_pid_is_polled_with_a_deadline(self):
        device = AndroidDevice('fixture')
        with patch.object(device, 'shell', side_effect=['', subprocess.CalledProcessError(1, ['pidof']), '123']), patch('android_benchmark_device.time.sleep'):
            self.assertEqual(device.wait_for_pid('com.scene', timeout=1), '123')
        with patch.object(device, 'shell', return_value=''), patch('android_benchmark_device.time.monotonic', side_effect=[0, 0, 2]), patch('android_benchmark_device.time.sleep'):
            with self.assertRaisesRegex(ValueError, 'PID'):
                device.wait_for_pid('com.scene', timeout=1)

    def test_first_gesture_requires_visible_content_motion(self):
        route = json.loads((Path(__file__).parent / 'android/routes/huawei-showcase.json').read_text())
        for changed in [False, True]:
            with self.subTest(changed=changed):
                device = Mock(spec=AndroidDevice)
                device.screenshot.side_effect = [{'motion_pixels_sha256': 'start'},
                                                {'motion_pixels_sha256': 'end' if changed else 'start'}]
                report = {}
                if changed:
                    benchmark.verify_first_gesture(device, route, self.root, report)
                    self.assertTrue(report['first_gesture']['changed'])
                else:
                    with self.assertRaisesRegex(ValueError, 'first gesture'):
                        benchmark.verify_first_gesture(device, route, self.root, report)
                device.shell.assert_called_once_with('input', 'swipe', route['x'], route['y_start'],
                                                    route['x'], route['y_end'], route['duration_ms'])

    def test_recorder_rejects_early_exit_and_recovers_its_pid_before_cleanup(self):
        for early in [False, True]:
            with self.subTest(early=early):
                device = Mock(spec=AndroidDevice)
                device.command = ['adb', '-s', 'fixture']
                process = Mock()
                process.poll.side_effect = [0 if early else None, 0]
                process.communicate.return_value = (b'recorder output', None)
                process.returncode = 0
                report = {}
                recording = AndroidRecording(device, self.root, [100, 100], report, 2_000_000)
                recording.process = process
                device.shell.return_value = '123'
                device.run.side_effect = lambda *parts, **_options: Path(parts[-1]).write_bytes(b'video')
                probe = {'streams': [{'width': 100, 'height': 100, 'nb_read_frames': '30'}]}
                with patch('android_benchmark_video.checked_command', return_value=json.dumps(probe).encode()):
                    if early:
                        with self.assertRaisesRegex(BaseExceptionGroup, 'Recording failed'):
                            recording.finish()
                    else:
                        recording.finish()
                device.stop_owned_process.assert_called_once_with('123', recording.remote, first_signal='INT')
                removed = {call.args[-1] for call in device.shell.call_args_list if call.args[0] == 'rm'}
                self.assertEqual(removed, {recording.remote, recording.pid_file})
                self.assertTrue(Path(report['video']['path']).is_file())

    def test_unavailable_device_recorder_is_rejected_before_launch(self):
        device = Mock(spec=AndroidDevice)
        device.command = ['adb', '-s', 'fixture']
        device.shell.side_effect = subprocess.CalledProcessError(127, ['screenrecord'])
        recording = AndroidRecording(device, self.root, [100, 100], {}, 2_000_000)
        reader, writer = os.pipe()
        os.write(writer, b'123\n')
        os.close(writer)
        with os.fdopen(reader, 'rb') as output:
            with patch('android_benchmark_video.subprocess.Popen', return_value=Mock(stdout=output)) as launch:
                with self.assertRaises(subprocess.CalledProcessError):
                    recording.start()
                launch.assert_not_called()

    def test_scrcpy_recording_waits_for_readiness_and_child_completion(self):
        executable = self.root / 'scrcpy'
        executable.write_text('#!' + sys.executable + '\n' + '''
import sys,time
from pathlib import Path
output=next(argument.split('=',1)[1] for argument in sys.argv if argument.startswith('--record='))
Path(output).write_bytes(bytes.fromhex('000000206674797069736f6d0000020069736f6d69736f32617663316d7034310000000866726565000000006d646174'))
time.sleep(0.3)
''')
        executable.chmod(0o755)
        report = {}
        recording = ScrcpyRecording(SimpleNamespace(serial='fixture'), self.root, [100, 100], report, 2_000_000)
        with patch.dict(os.environ, {'PATH': str(self.root) + os.pathsep + os.environ['PATH']}):
            recording.start()
        self.assertIsNone(recording.process.poll())
        probe = {'streams': [{'width': 100, 'height': 100, 'nb_read_frames': '30'}]}
        with patch('android_benchmark_video.checked_command', return_value=json.dumps(probe).encode()):
            recording.finish()
        self.assertEqual(recording.process.returncode, 0)
        self.assertTrue(recording.log.closed)
        self.assertEqual(report['video']['sha256'], support.digest(self.root / 'scroll.mp4'))

    def test_scrcpy_early_exit_and_wrong_dimensions_reject_recordings(self):
        recording = ScrcpyRecording(SimpleNamespace(serial='fixture'), self.root, [100, 100], {}, 2_000_000)
        recording.process = Mock(returncode=0)
        recording.started_at = 0
        recording.process.poll.return_value = 0
        with patch('android_benchmark_video.inspect_recording'):
            with self.assertRaisesRegex(BaseExceptionGroup, 'Recording failed'):
                recording.finish()
        recording.process.terminate.assert_not_called()
        video = self.root / 'scroll.mp4'
        video.write_bytes(b'video')
        for streams in [[], [{'width': 100, 'height': 99, 'nb_read_frames': '30'}],
                        [{'width': 100, 'height': 100, 'nb_read_frames': '1'}]]:
            with self.subTest(streams=streams):
                with patch('android_benchmark_video.checked_command', return_value=json.dumps({'streams': streams}).encode()):
                    with self.assertRaisesRegex(ValueError, 'full-size route frames'):
                        inspect_recording(video, [100, 100], {})

    def test_apk_provenance_rejects_modified_code_assets_and_unmeasured_abis(self):
        apk = self.root / 'app.apk'
        member = 'lib/arm64-v8a/libapp.so'
        with zipfile.ZipFile(apk, 'w') as archive:
            archive.writestr(member, b'native')
            archive.writestr('assets/scene', b'picture')
        with zipfile.ZipFile(apk) as archive:
            proof = {'apk_sha256': support.digest(apk), 'build': {
                'native_sha256': artifacts.hashlib.sha256(b'native').hexdigest()},
                'payload': artifacts.apk_payload(archive, member)}
        artifacts.verify_apk(apk, proof, member)
        for changed in ['native', 'asset', 'abi']:
            modified = self.root / f'{changed}.apk'
            with zipfile.ZipFile(modified, 'w') as archive:
                archive.writestr(member, b'wrong' if changed == 'native' else b'native')
                archive.writestr('assets/scene', b'wrong' if changed == 'asset' else b'picture')
                if changed == 'abi':
                    archive.writestr('lib/armeabi-v7a/libapp.so', b'unmeasured')
            with self.assertRaises(ValueError):
                artifacts.verify_apk(modified, proof, member)
            updated = proof | {'apk_sha256': support.digest(modified)}
            with self.assertRaises(ValueError):
                artifacts.verify_apk(modified, updated, member)


if __name__ == '__main__':
    unittest.main()
