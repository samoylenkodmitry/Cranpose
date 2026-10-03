import argparse
import json
from pathlib import Path
import signal
import uuid

from android_benchmark import run_leg, stage_apk, validate_route
from android_benchmark_artifacts import verify_apk, verify_build, verify_helper
from android_benchmark_device import AndroidDevice
from android_benchmark_support import device_lock, digest, interrupted, run_reported
from android_benchmark_video import AndroidRecording, ScrcpyRecording


def probe(args, report):
    route = json.loads(args.route.read_text())
    validate_route(route)
    proof = json.loads(args.proof.read_text())
    if proof['status'] != 'complete':
        raise ValueError('APK provenance did not complete')
    verify_build(proof['build'], Path(proof['build_directory']))
    apk = args.proof.parent / proof['apk']
    verify_apk(apk, proof, proof['native_member'])
    properties = json.loads(args.properties)
    if not isinstance(properties, dict):
        raise ValueError('Properties must be an object')
    tooling = Path(__file__).parent
    helper = verify_helper(args.dex, tooling / 'android/CranposeBenchmarkRoute.java',
                           args.dex.with_name('route.json'), 'dex_sha256')
    ocr = verify_helper(args.ocr, tooling / 'android/recognize_text.swift',
                        args.ocr.with_suffix('.json'), 'executable_sha256')
    device = AndroidDevice(args.serial, args.adb, args.ocr, args.output / 'endpoints')
    recording_type = ScrcpyRecording if args.record_backend == 'scrcpy' else AndroidRecording
    recorder = (recording_type(device, args.output, route['size'], report, 12000000)
                if args.record else None)
    remote = '/data/local/tmp/cranpose-probe-' + uuid.uuid4().hex + '.apk'
    report.update(serial=args.serial, properties=properties, helper=helper, ocr=ocr,
                  route_sha256=digest(args.route), diagnostic=True)

    def measure():
        stage_apk(device, route['package'], apk, remote, proof['apk_sha256'], 120)
        try:
            device.configure_properties(properties)
        finally:
            report['saved_properties'] = device.saved_properties
        run_leg(device, route, apk, proof, args.dex, args.output, report, remote, recorder)
        report['acceptance_eligible'] = False

    def capture_logs():
        if 'pid' not in report:
            return
        report['diagnostic_log'] = 'logcat.txt'
        (args.output / 'logcat.txt').write_text(device.shell(
            'logcat', '-d', '--pid=' + report['pid'], '-T', report['launch_time']))

    with device_lock(args.serial):
        run_reported(args.output / 'report.json', report, measure,
                     [lambda: recorder.finish() if recorder else None,
                      capture_logs, device.restore_properties,
                      lambda: device.shell('am', 'force-stop', route['package']),
                      lambda: device.shell('rm', '-f', remote)])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--serial', required=True)
    parser.add_argument('--adb', default='adb')
    parser.add_argument('--proof', type=Path, required=True)
    parser.add_argument('--route', type=Path, required=True)
    parser.add_argument('--dex', type=Path, required=True)
    parser.add_argument('--ocr', type=Path, required=True)
    parser.add_argument('--properties', default='{}')
    parser.add_argument('--record', action='store_true')
    parser.add_argument('--record-backend', choices=['screenrecord', 'scrcpy'], default='screenrecord')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    signal.signal(signal.SIGTERM, interrupted)
    report = {}
    run_reported(args.output / 'report.json', report, lambda: probe(args, report))


if __name__ == '__main__':
    main()
