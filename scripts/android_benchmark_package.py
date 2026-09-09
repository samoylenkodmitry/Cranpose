import argparse
import json
import signal
from pathlib import Path
import zipfile

from android_benchmark_artifacts import apk_payload, verify_apk, verify_build
from android_benchmark_support import checked_command, digest, interrupted, run_reported


def package(args, report):
    build = json.loads(args.build_report.read_text())
    verify_build(build, args.build_report.parent)
    native = args.build_report.parent / build['native']
    member = f'lib/{build["abi"]}/{native.name}'
    unsigned = args.output / 'unaligned.apk'
    apk = args.output / 'app.apk'
    with zipfile.ZipFile(args.template) as source, zipfile.ZipFile(unsigned, 'w') as target:
        if len(source.namelist()) != len(set(source.namelist())):
            raise ValueError('Template APK contains duplicate members')
        payload = apk_payload(source, member)
        for item in source.infolist():
            if item.filename in payload:
                target.writestr(item, source.read(item))
        target.write(native, member, compress_type=zipfile.ZIP_STORED)
    zipalign = str(args.build_tools / 'zipalign')
    apksigner = str(args.build_tools / 'apksigner')
    checked_command([zipalign, '-P', '16', '4', str(unsigned), str(apk)])
    checked_command([apksigner, 'sign', '--ks', str(args.keystore),
                     '--ks-key-alias', args.key_alias, '--ks-pass', 'env:' + args.password_env,
                     '--key-pass', 'env:' + args.password_env, str(apk)])
    checked_command([apksigner, 'verify', str(apk)])
    checked_command([zipalign, '-c', '-P', '16', '4', str(apk)])
    report.update(apk='app.apk', apk_sha256=digest(apk), template_sha256=digest(args.template),
                  native_member=member, build=build, build_directory=str(args.build_report.parent.resolve()),
                  payload=payload)
    verify_apk(apk, report, member)


def main():
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser()
    parser.add_argument('--build-report', type=Path, required=True)
    parser.add_argument('--template', type=Path, required=True)
    parser.add_argument('--build-tools', type=Path, required=True)
    parser.add_argument('--keystore', type=Path, required=True)
    parser.add_argument('--key-alias', required=True)
    parser.add_argument('--password-env', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {}
    run_reported(args.output / 'apk.json', report, lambda: package(args, report))


if __name__ == '__main__':
    main()
