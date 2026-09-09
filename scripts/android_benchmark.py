import argparse
import json
import math
from pathlib import Path
import re
import signal
import statistics
import subprocess
import time
import uuid

from android_benchmark_artifacts import snapshot_source, source_inventory, verify_apk, verify_build
from android_benchmark_device import AndroidDevice, surface_frames, verify_route_window
from android_benchmark_support import checked_command, device_lock, digest, error_details, interrupted, run_reported, write_report
from android_benchmark_video import AndroidRecording, ScrcpyRecording


def validate_pair(proofs, variant_source='framework'):
    first, second = proofs
    for key in ['payload']:
        if first[key] != second[key]:
            raise ValueError('Compared APKs differ in ' + key)
    for key in ['abi', 'features', 'toolchain', 'cargo', 'ndk', 'settings', 'lock_sha256']:
        if first['build'][key] != second['build'][key]:
            raise ValueError('Compared builds differ in ' + key)
    fixed_source = {'framework': 'app', 'app': 'framework'}[variant_source]
    if first['build']['sources'][fixed_source]['inventory'] != second['build']['sources'][fixed_source]['inventory']:
        raise ValueError('Compared builds differ in ' + fixed_source + ' sources')


def validate_route(route):
    if route['kind'] not in {'scroll', 'game'} or not route['package'] or not route['activity']:
        raise ValueError('Route needs a supported scene and explicit component')
    if route['kind'] == 'scroll' and (not route['start_text'] or not route['end_text']
                                     or route['start_text'] == route['end_text'] or route['count'] <= 0):
        raise ValueError('Scroll route needs distinct positive start/end checks and input')
    if route['kind'] == 'game' and (not route['ready_log'] or route['count'] != 0):
        raise ValueError('Game window requires a positive launch marker and no injected gestures')
    if (route['duration_ms'] < 1 or route['period_ms'] < route['duration_ms']
            or route['window_ms'] <= 0 or route['window_ms'] < route['count'] * route['period_ms']
            or not 0 <= route['timing_tolerance_ms'] < route['period_ms']):
        raise ValueError('Invalid route timing')
    for x, y in [(route['x'], route['y_start']), (route['x'], route['y_end']),
                 *[step['tap'] for step in route.get('setup', [])]]:
        if not 0 <= x < route['size'][0] or not 0 <= y < route['size'][1]:
            raise ValueError('Route input falls outside the verified display')
    left, top, right, bottom = route['motion_region']
    if not 0 <= left < right <= route['size'][0] or not 0 <= top < bottom <= route['size'][1]:
        raise ValueError('Motion region falls outside the verified display')


def wait_ready(device, route, output):
    deadline = time.monotonic() + route.get('startup_timeout_s', 30)
    failure = None
    while time.monotonic() < deadline:
        try:
            if route['kind'] == 'scroll':
                return device.endpoint(route['package'], route['start_text'], route['size'],
                                       route.get('start_region'), route.get('text_source', 'accessibility'))
            logs = device.shell('logcat', '-d', '--pid=' + output['pid'], '-T', output['launch_time'])
            match = re.search(route['ready_log'], logs)
            if match:
                return match[0]
            failure = ValueError('Game startup marker was not observed')
        except (ValueError, subprocess.CalledProcessError) as error:
            failure = error
        time.sleep(0.25)
    raise RuntimeError('Application did not reach its verified start state') from failure


def stage_apk(device, package, apk, remote, expected_hash, timeout):
    device.wake()
    installed = device.installed_apk(package)
    if installed and installed['sha256'] == expected_hash:
        device.shell('cp', installed['path'], remote)
    else:
        device.run('push', str(apk), remote, timeout=timeout)
    if device.shell('sha256sum', remote).split()[0] != expected_hash:
        raise ValueError('Staged APK differs from its build proof')


def install_staged(device, package, source, expected_hash, report):
    device.wake()
    installed = device.installed_apk(package)
    report['installation'] = {'reused': bool(installed and installed['sha256'] == expected_hash)}
    if not report['installation']['reused']:
        try:
            result = device.shell('pm', 'install', '-r', source, timeout=120)
            report['installation']['output'] = result
            if 'Success' not in result:
                raise ValueError('APK installation did not report success: ' + result)
        except subprocess.TimeoutExpired as error:
            report['installation']['timeout'] = error_details(error)
        installed = device.installed_apk(package)
    actual = installed['sha256'] if installed else None
    report['installation']['installed_sha256'] = actual
    if actual != expected_hash:
        raise ValueError('Installed APK does not match the measured build')


def launch_route(device, route, report):
    package = route['package']
    device.shell('am', 'force-stop', package)
    report['launch_time'] = device.shell('date', '+%m-%d %H:%M:%S.000').strip()
    device.wake()
    device.shell('am', 'start', '-W', '-n', package + '/' + route['activity'], *route.get('launch_args', []))
    report['pid'] = device.wait_for_pid(package, timeout=route.get('startup_timeout_s', 30))
    report['setup_validation'] = []
    for step in route.get('setup', []):
        ready = route | {'kind': 'scroll', 'start_text': step['visible_text'], 'start_region': step.get('region'),
                         'text_source': step.get('text_source', 'accessibility')}
        report['setup_validation'].append(wait_ready(device, ready, report))
        device.wake()
        device.shell('input', 'tap', *step['tap'])
    report['start_validation'] = wait_ready(device, route, report)


def verify_first_gesture(device, route, destination, report):
    probe = {'before': device.screenshot(destination / 'first-gesture-before.png', route['motion_region'])}
    report['first_gesture'] = probe
    device.wake()
    device.shell('input', 'swipe', route['x'], route['y_start'], route['x'], route['y_end'], route['duration_ms'])
    probe['after'] = device.screenshot(destination / 'first-gesture-after.png', route['motion_region'])
    probe['changed'] = probe['before']['motion_pixels_sha256'] != probe['after']['motion_pixels_sha256']
    if not probe['changed']:
        raise ValueError('The first gesture did not move visible content')


def run_leg(device, route, apk, proof, dex, destination, report, installed_source, recorder=None):
    package = route['package']
    report.update(apk_sha256=proof['apk_sha256'], native_sha256=proof['build']['native_sha256'], route=route)
    verify_apk(apk, proof, proof['native_member'])
    install_staged(device, package, installed_source, proof['apk_sha256'], report)
    launch_route(device, route, report)
    report['start_image'] = device.screenshot(destination / 'start.png', route['motion_region'])
    if report['start_image']['size'] != route['size']:
        raise ValueError('Device display size differs from the verified route')
    density = device.shell('wm', 'density')
    if int(re.findall(r'density: (\d+)', density)[-1]) != route['density']:
        raise ValueError('Device density differs from the verified route')
    if route['kind'] == 'scroll':
        verify_first_gesture(device, route, destination, report)
        launch_route(device, route, report)
    if recorder:
        recorder.start()
    windows = [route]
    if 'return' in route:
        windows.append(route | {'start_text': route['end_text'], 'end_text': route['start_text'],
                                'start_region': route.get('end_region'), 'end_region': route.get('start_region')}
                       | route['return'])
    report['windows'] = []
    for index, window_route in enumerate(windows, 1):
        validate_route(window_route)
        directory = destination / f'window-{index}'
        directory.mkdir()
        window = {'pid': report['pid'], 'launch_time': report['launch_time'], 'route': window_route,
                  'recording': recorder is not None}
        report['windows'].append(window)
        run_reported(directory / 'report.json', window,
                     lambda: run_window(device, window_route, dex, directory, window),
                     [lambda: stop_route(device, directory, window),
                      lambda: device.shell('dumpsys', 'SurfaceFlinger', '--timestats', '-disable')])
    elapsed = sum(window['elapsed_s'] for window in report['windows'])
    frames = sum(window['layer']['frames'] for window in report['windows'])
    report.update(elapsed_s=elapsed, frames=frames, fps=frames / elapsed,
                  before=report['windows'][0]['before'], after=report['windows'][-1]['after'],
                  acceptance_eligible=recorder is None)


def run_window(device, route, dex, destination, report):
    package = route['package']
    report['start_validation'] = wait_ready(device, route, report)
    report['start_image'] = device.screenshot(destination / 'start.png', route['motion_region'])
    report['foreground_before'] = device.require_foreground(package, report['pid'])
    report['before'] = device.state()
    remote_dex = '/data/local/tmp/cranpose-benchmark-' + digest(dex) + '.dex'
    device.run('push', str(dex), remote_dex)
    if device.shell('sha256sum', remote_dex).split()[0] != digest(dex):
        raise ValueError('Device route bytecode differs from the verified helper')
    report['run_id'] = uuid.uuid4().hex
    command = ['env', 'CLASSPATH=' + remote_dex, 'app_process', '/system/bin', 'CranposeBenchmarkRoute',
               *[route[key] for key in ['x', 'y_start', 'y_end', 'count', 'duration_ms', 'period_ms', 'window_ms']],
               report['run_id']]
    device.wake()
    output = device.shell(*command, timeout=route['window_ms'] / 1000 + 30,
                          output=destination / 'device-route.log')
    report['after'] = device.state()
    report['foreground_after'] = device.require_foreground(package, report['pid'])
    capture_end(device, route, destination, report)
    if report['start_image']['motion_pixels_sha256'] == report['end_image']['motion_pixels_sha256']:
        raise ValueError('Scene did not change during the measurement')
    elapsed, stats = verify_route_window(output, *[route[key] for key in [
        'count', 'duration_ms', 'period_ms', 'window_ms', 'timing_tolerance_ms']])
    (destination / 'surfaceflinger.txt').write_text(stats)
    layer = surface_frames(stats, package)
    report.update(elapsed_s=elapsed, layer=layer, fps=layer['frames'] / elapsed,
                  acceptance_eligible=not report['recording'],
                  timing='device uptime; excludes SurfaceFlinger control IPC')


def capture_end(device, route, destination, report):
    timeout = route.get('endpoint_timeout_s', 0)
    if not math.isfinite(timeout) or timeout < 0:
        raise ValueError('Endpoint timeout must be finite and nonnegative')
    started = time.monotonic()
    deadline = started + timeout
    report['endpoint_attempts'] = []
    while True:
        index = len(report['endpoint_attempts']) + 1
        path = destination / ('end.png' if index == 1 else f'end-{index}.png')
        report['end_image'] = device.screenshot(path, route['motion_region'])
        attempt = {'image': str(path), 'pixels': report['end_image']}
        report['endpoint_attempts'].append(attempt)
        report['endpoint_settle_elapsed_s'] = time.monotonic() - started
        if route['kind'] != 'scroll':
            return
        try:
            report['end_validation'] = device.endpoint(route['package'], route['end_text'], route['size'],
                                                       route.get('end_region'), route.get('text_source', 'accessibility'),
                                                       image_path=path)
            return
        except ValueError as error:
            attempt['error'] = str(error)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise
            time.sleep(min(0.25, remaining))


def stop_route(device, directory, report):
    log = directory / 'device-route.log'
    if not log.exists() or 'run_id' not in report:
        return
    match = re.search(r'route_pid=(\d+) run_id=' + re.escape(report['run_id']), log.read_text())
    if not match:
        return
    device.stop_owned_process(match[1], report['run_id'])


def sequence(args, report):
    if args.transfer_timeout_seconds <= 0:
        raise ValueError('Transfer timeout must be positive')
    report['transfer_timeout_seconds'] = args.transfer_timeout_seconds
    tooling = Path(__file__).parent
    report['tooling'] = {'archive': 'tooling.tar.gz',
                         'sha256': snapshot_source(tooling, args.output / 'tooling.tar.gz'),
                         'inventory': source_inventory(tooling)}
    route = json.loads(args.route.read_text())
    validate_route(route)
    helper = json.loads(args.dex.with_name('route.json').read_text())
    if (helper['dex_sha256'] != digest(args.dex)
            or helper['source_sha256'] != digest(Path(__file__).parent / 'android/CranposeBenchmarkRoute.java')):
        raise ValueError('Route helper does not match the versioned source and build proof')
    inputs = [path.resolve() for path in [args.a, args.b]]
    proofs = [json.loads(path.read_text()) for path in inputs]
    for path, proof in zip(inputs, proofs):
        if proof['status'] != 'complete':
            raise ValueError('APK provenance did not complete')
        verify_build(proof['build'], Path(proof['build_directory']))
        verify_apk(path.parent / proof['apk'], proof, proof['native_member'])
    validate_pair(proofs, args.variant_source)
    report['variant_source'] = args.variant_source
    report.update(serial=args.serial, route_sha256=digest(args.route), helper=helper, legs=[])
    if args.ocr:
        proof = json.loads(args.ocr.with_suffix('.json').read_text())
        if (proof['executable_sha256'] != digest(args.ocr)
                or proof['source_sha256'] != digest(tooling / 'android/recognize_text.swift')):
            raise ValueError('OCR helper does not match its source build')
        report['ocr'] = proof
    device = AndroidDevice(args.serial, args.adb, args.ocr, args.output / 'endpoints')
    remote_apks = {proof['apk_sha256']: (path.parent / proof['apk'],
                                         '/data/local/tmp/cranpose-benchmark-' + uuid.uuid4().hex + '.apk')
                   for path, proof in zip(inputs, proofs)}
    with device_lock(args.serial):
        def measure():
            for sha256, (apk, remote) in remote_apks.items():
                stage_apk(device, route['package'], apk, remote, sha256, args.transfer_timeout_seconds)
            try:
                device.configure_properties({})
            finally:
                report['saved_properties'] = device.saved_properties
            for index, slot in enumerate('AB' if args.record else 'ABABBABA', 1):
                source = 0 if slot == 'A' else 1
                destination = args.output / f'{index}-{slot}'
                destination.mkdir()
                leg = {'slot': slot, 'index': index}
                report['legs'].append(leg)
                recorder = None
                if args.record:
                    recording_type = ScrcpyRecording if args.record_backend == 'scrcpy' else AndroidRecording
                    options = {'duration_seconds': args.video_seconds} if args.record_backend == 'scrcpy' else {}
                    recorder = recording_type(device, destination, route['size'], leg, args.video_bit_rate, **options)
                run_reported(destination / 'report.json', leg,
                             lambda: run_leg(device, route, inputs[source].parent / proofs[source]['apk'],
                                             proofs[source], args.dex, destination, leg,
                                             remote_apks[proofs[source]['apk_sha256']][1], recorder),
                             [lambda: stop_route(device, destination, leg),
                              lambda: device.shell('dumpsys', 'SurfaceFlinger', '--timestats', '-disable'),
                              lambda: recorder.finish() if recorder else None,
                              lambda: device.shell('am', 'force-stop', route['package'])])
                write_report(args.output / 'report.json', report)
                print(json.dumps({key: leg[key] for key in ['slot', 'index', 'fps', 'elapsed_s']}), flush=True)
            report['mean_fps'] = {slot: statistics.mean(leg['fps'] for leg in report['legs'] if leg['slot'] == slot)
                                  for slot in ['A', 'B']}
            if source_inventory(tooling) != report['tooling']['inventory']:
                raise ValueError('Measurement tooling changed during the sequence')
            report['acceptance_eligible'] = not args.record
        run_reported(args.output / 'report.json', report, measure,
                     [device.restore_properties, *[lambda path=path: device.shell('rm', '-f', path)
                                                    for _, path in remote_apks.values()]])


def build_route(args):
    args.output.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).parent / 'android/CranposeBenchmarkRoute.java'
    classes = args.output / 'classes'
    classes.mkdir()
    checked_command(['javac', '--release', '8', '-classpath', str(args.android_jar), '-d', str(classes), str(source)])
    checked_command([str(args.d8), '--output', str(args.output), str(classes / 'CranposeBenchmarkRoute.class')])
    write_report(args.output / 'route.json', {'source_sha256': digest(source),
                                             'dex_sha256': digest(args.output / 'classes.dex'),
                                             'android_jar_sha256': digest(args.android_jar)})


def build_ocr(args):
    args.output.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).parent / 'android/recognize_text.swift'
    executable = args.output / 'recognize-text'
    checked_command(['swiftc', '-O', str(source), '-o', str(executable)], timeout=120)
    write_report(executable.with_suffix('.json'), {'source_sha256': digest(source),
                                                  'executable_sha256': digest(executable)})


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    route = commands.add_parser('build-route')
    route.add_argument('--android-jar', type=Path, required=True)
    route.add_argument('--d8', type=Path, required=True)
    route.add_argument('--output', type=Path, required=True)
    ocr = commands.add_parser('build-ocr')
    ocr.add_argument('--output', type=Path, required=True)
    measure = commands.add_parser('measure')
    measure.add_argument('--serial', required=True)
    measure.add_argument('--adb', default='adb')
    measure.add_argument('--transfer-timeout-seconds', type=int, default=120)
    measure.add_argument('--route', type=Path, required=True)
    measure.add_argument('--dex', type=Path, required=True)
    measure.add_argument('--ocr', type=Path)
    measure.add_argument('--record', action='store_true')
    measure.add_argument('--record-backend', choices=['screenrecord', 'scrcpy'], default='screenrecord')
    measure.add_argument('--video-seconds', type=int, choices=range(1, 181), default=60, metavar='1..180')
    measure.add_argument('--video-bit-rate', type=int, default=2_000_000)
    measure.add_argument('--a', type=Path, required=True)
    measure.add_argument('--b', type=Path, required=True)
    measure.add_argument('--variant-source', choices=['framework', 'app'], default='framework')
    measure.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, interrupted)
    if args.command == 'build-route':
        build_route(args)
    elif args.command == 'build-ocr':
        build_ocr(args)
    else:
        args.output.mkdir(parents=True, exist_ok=False)
        report = {}
        run_reported(args.output / 'report.json', report, lambda: sequence(args, report))


if __name__ == '__main__':
    main()
