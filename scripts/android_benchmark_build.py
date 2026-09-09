import argparse
import json
import signal
import os
from pathlib import Path
import shutil
import tomllib

from android_benchmark_artifacts import build_cache, extract_source, snapshot_source, source_inventory
from android_benchmark_support import checked_command, digest, interrupted, run_reported


def framework_packages(root):
    packages = {}
    for manifest in (root / 'crates').rglob('Cargo.toml'):
        name = tomllib.loads(manifest.read_text()).get('package', {}).get('name')
        if name:
            packages[name] = manifest.parent
    return packages


def native_artifact(exported, package, abi, build_output):
    warnings = [line for line in build_output.decode(errors='replace').splitlines()
                if line.startswith('warning:')]
    if warnings:
        raise ValueError('Build emitted warnings: ' + '\n'.join(warnings))
    names = {target['name'] for target in package['targets'] if 'cdylib' in target['crate_types']}
    if len(names) != 1:
        raise ValueError('Expected one native library target in Cargo metadata')
    artifact = exported / abi / ('lib' + names.pop() + '.so')
    if not artifact.is_file():
        raise ValueError('cargo-ndk did not export the selected library: ' + str(artifact))
    return artifact


def build_settings(args, environment):
    names = {'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
             'CC', 'CXX', 'AR', 'CFLAGS', 'CXXFLAGS', 'CPPFLAGS', 'LDFLAGS'}
    flags = {name: value for name, value in environment.items()
             if name in names or name.startswith(('CARGO_PROFILE_', 'CARGO_TARGET_'))
             and name != 'CARGO_TARGET_DIR'}
    cargo_home = Path(environment.get('CARGO_HOME', Path.home() / '.cargo'))
    configs = {name: digest(cargo_home / name) for name in ['config', 'config.toml']
               if (cargo_home / name).is_file()}
    ndk = Path(environment['ANDROID_NDK_HOME'])
    return {'package': args.package, 'platform': args.platform, 'profile': 'release',
            'flags': flags, 'cargo_config_sha256': configs,
            'ndk_properties_sha256': digest(ndk / 'source.properties')}


def write_framework_overrides(destination, packages, selected):
    destination.write_text('[patch.crates-io]\n' + ''.join(
        json.dumps(name) + ' = { path = ' + json.dumps(str(packages[name])) + ' }\n'
        for name in sorted(selected)))


def build(args, report):
    cache = build_cache(args.cache)
    report['cache'] = str(cache)
    sources = {}
    for name, original in [('framework', args.framework), ('app', args.app)]:
        archive = (args.output if name == 'framework' else cache) / f'{name}.tar.gz'
        source_hash = snapshot_source(original, archive)
        inventory = extract_source(archive, cache / name)
        sources[name] = {'archive': archive.name, 'sha256': source_hash, 'inventory': inventory}
    report['sources'] = sources
    app = cache / 'app'
    environment = dict(os.environ, CARGO_TARGET_DIR=str(args.target_dir.resolve()))
    common = ['--locked', '--no-default-features', '--features', args.features]
    packages = framework_packages(cache / 'framework')
    selected = set()
    for manifest in [app / 'Cargo.toml', app / '.cargo/config.toml', app / '.cargo/config']:
        if manifest.is_file():
            patches = tomllib.loads(manifest.read_text()).get('patch', {}).get('crates-io', {})
            selected |= patches.keys() & packages.keys()
    overrides = cache / 'framework.toml'
    write_framework_overrides(overrides, packages, selected)
    metadata_command = ['cargo', 'metadata', '--format-version=1', *common, '--config', str(overrides)]
    if selected:
        metadata_command.remove('--locked')
    initial_metadata = json.loads(checked_command(metadata_command, cwd=app, env=environment,
                                                 output=args.output / 'initial-metadata.log'))
    selected |= {package['name'] for package in initial_metadata['packages']} & packages.keys()
    if not selected:
        raise ValueError('Application resolves no framework packages')
    write_framework_overrides(overrides, packages, selected)
    if '--locked' in metadata_command:
        metadata_command.remove('--locked')
    metadata_command.append('--offline')
    metadata = json.loads(checked_command(metadata_command, cwd=app, env=environment,
                                          output=args.output / 'resolved-metadata.log'))
    resolved = {package['name']: package['manifest_path'] for package in metadata['packages']
                if package['source'] is None}
    if any(not Path(path).is_relative_to(cache) for path in resolved.values()):
        raise ValueError('Application resolves source outside its immutable snapshot: ' + str(resolved))
    if any(Path(resolved[name]).parent != packages[name] for name in selected & resolved.keys()):
        raise ValueError('Application does not resolve the archived framework')
    app_archive = args.output / 'app.tar.gz'
    sources['app'] = {'archive': app_archive.name, 'sha256': snapshot_source(app, app_archive),
                      'inventory': source_inventory(app)}
    package = next(package for package in metadata['packages'] if package['name'] == args.package)
    exported = cache / 'native'
    command = ['cargo', 'ndk', '--platform', str(args.platform), '-t', args.abi,
               '--output-dir', str(exported),
               'build', *common, '--release', '-p', args.package, '--lib',
               '--config', str(overrides)]
    report.update(command=command, resolved=resolved,
                  toolchain=checked_command(['rustc', '-Vv'], cwd=app, env=environment).decode(),
                  cargo=checked_command(['cargo', '-V'], cwd=app, env=environment).decode(),
                  settings=build_settings(args, environment),
                  ndk=environment['ANDROID_NDK_HOME'], features=args.features, abi=args.abi)
    log = args.output / 'build.log'
    checked_command(command, cwd=app, env=environment, timeout=args.timeout, output=log)
    artifact = native_artifact(exported, package, args.abi, log.read_bytes())
    for name, source in sources.items():
        if source_inventory(cache / name) != source['inventory']:
            raise ValueError('Build changed archived sources: ' + name)
    native = args.output / artifact.name
    shutil.copyfile(artifact, native)
    report.update(native=artifact.name, native_sha256=digest(native),
                  lock_sha256=digest(app / 'Cargo.lock'))


def main():
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser()
    parser.add_argument('--framework', type=Path, required=True)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--package', required=True)
    parser.add_argument('--features', required=True)
    parser.add_argument('--abi', required=True, choices=['arm64-v8a', 'armeabi-v7a', 'x86_64', 'x86'])
    parser.add_argument('--platform', type=int, required=True)
    parser.add_argument('--cache', type=Path, default=Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'cranpose/benchmarks')
    parser.add_argument('--target-dir', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=1800)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {}
    run_reported(args.output / 'build.json', report, lambda: build(args, report))


if __name__ == '__main__':
    main()
