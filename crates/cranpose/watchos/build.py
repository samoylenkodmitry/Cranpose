#!/usr/bin/env python3
"""Build a watchOS app from a shared Rust composable.

All generated bridge and Apple host files stay in the output directory. The
application needs an ordinary library target and no native Apple source files.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tomllib


def run(args, *, cwd=None, env=None):
    print('+', ' '.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd, env=env, check=True)


def output(args):
    return subprocess.check_output(args, text=True).strip()


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists() or path.read_text() != content:
        path.write_text(content)


def rust_bridge(entry, application_id):
    return '''pub struct Application(cranpose::watchos::Application);

impl Application {
    pub fn resize(&mut self, width: u32, height: u32, density: f32) -> Result<(), cranpose::watchos::SurfaceError> {
        self.0.resize(width, height, density)
    }
    pub fn tick(&mut self, elapsed: u64) -> bool { self.0.tick(elapsed) }
    pub fn pixels(&self) -> &[u8] { self.0.pixels() }
    pub fn width(&self) -> u32 { self.0.width() }
    pub fn height(&self) -> u32 { self.0.height() }
    pub fn touch(&mut self, phase: u8, x: f32, y: f32) { self.0.touch(phase, x, y); }
    pub fn crown(&mut self, delta: f32, elapsed: u64) -> bool { self.0.crown(delta, elapsed) }
    pub fn set_active(&mut self, active: bool) { self.0.set_active(active); }
}

#[cxx::bridge(namespace = "cranpose_watchos")]
mod ffi {
    extern "Rust" {
        type Application;
        fn create_application(width: u32, height: u32, density: f32) -> Result<Box<Application>>;
        fn resize(self: &mut Application, width: u32, height: u32, density: f32) -> Result<()>;
        fn tick(self: &mut Application, elapsed_nanos: u64) -> bool;
        fn pixels(self: &Application) -> &[u8];
        fn width(self: &Application) -> u32;
        fn height(self: &Application) -> u32;
        fn touch(self: &mut Application, phase: u8, x: f32, y: f32);
        fn crown(self: &mut Application, delta: f32, elapsed_millis: u64) -> bool;
        fn set_active(self: &mut Application, active: bool);
    }
}

pub fn create_application(width: u32, height: u32, density: f32) -> Result<Box<Application>, Box<dyn std::error::Error>> {
    cranpose::host::set_application_id(''' + json.dumps(application_id) + ''')?;
    Ok(Box::new(Application(cranpose::watchos::Application::new(width, height, density, application::''' + entry + ''')?)))
}
'''


def compile_icon(icon, root, bundle, sdk_name, deployment_target):
    """Compiles `icon` into the bundle's asset catalog and returns the
    Info.plist entries that name it."""
    catalog = root / 'Assets.xcassets'
    iconset = catalog / 'AppIcon.appiconset'
    iconset.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(icon, iconset / 'icon.png')
    info = {'info': {'author': 'xcode', 'version': 1}}
    write(catalog / 'Contents.json', json.dumps(info))
    write(iconset / 'Contents.json', json.dumps({'images': [{
        'filename': 'icon.png', 'idiom': 'universal', 'platform': 'watchos', 'size': '1024x1024'}], **info}))
    partial = root / 'icon-info.plist'
    run(['xcrun', 'actool', catalog, '--compile', bundle, '--platform', sdk_name,
         '--minimum-deployment-target', deployment_target, '--target-device', 'watch',
         '--app-icon', 'AppIcon', '--output-partial-info-plist', partial])
    return plistlib.loads(partial.read_bytes())


def prepare_manifest(parser, runner, manifest, app_lock=None):
    path = runner / 'Cargo.toml'
    previous = tomllib.loads(path.read_text()) if path.exists() else {}
    cxx = previous.get('dependencies', {}).get('cxx')
    cxx_build = previous.get('build-dependencies', {}).get('cxx-build')
    if any(value is not None and not isinstance(value, str) for value in (cxx, cxx_build)):
        parser.error('generated bridge dependency configuration was modified')
    if cxx:
        manifest = manifest.replace('[profile.release]', f'cxx = {json.dumps(cxx)}\n\n[profile.release]')
    if cxx_build:
        manifest += f'\n[build-dependencies]\ncxx-build = {json.dumps(cxx_build)}\n'
    changed = not path.exists() or path.read_text() != manifest
    write(path, manifest)
    if app_lock and not (runner / 'Cargo.lock').exists():
        shutil.copyfile(app_lock, runner / 'Cargo.lock')
    if not cxx:
        run(['cargo', 'add', 'cxx@1'], cwd=runner)
    if not cxx_build:
        run(['cargo', 'add', 'cxx-build@1', '--build'], cwd=runner)
    if changed and cxx and cxx_build:
        run(['cargo', 'update', '--workspace'], cwd=runner)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--entry', help='Public composable path relative to the app crate, e.g. ui::OrbitApp')
    parser.add_argument('--bundle-id')
    parser.add_argument('--name')
    parser.add_argument('--companion', help='Bundle id of the iPhone app that carries this watch app; '
                        'without it the watch app stands alone')
    parser.add_argument('--icon', type=Path, help='Square PNG of at least 1024 pixels, relative to the '
                        'manifest; the watch shows it in a circle')
    parser.add_argument('--output', type=Path, required=True)
    framework = parser.add_mutually_exclusive_group()
    framework.add_argument('--framework-source', type=Path, help='Development-only Cranpose workspace override')
    framework.add_argument('--cranpose-version', help='Published Cranpose version with watchOS support')
    parser.add_argument('--features', help='Comma-separated application features; defaults stay disabled')
    parser.add_argument('--target', choices=['simulator', 'device'], default='simulator')
    parser.add_argument('--deployment-target', help='Defaults to 10.0 for simulator, 26.0 for arm64 devices')
    parser.add_argument('--sign', help='Device signing identity; provisioning is managed by the caller')
    parser.add_argument('--prepare-only', action='store_true')
    args = parser.parse_args()
    app_manifest = args.manifest.resolve()
    app = tomllib.loads(app_manifest.read_text())['package']
    metadata = app.get('metadata', {}).get('cranpose', {}).get('watchos', {})
    for field in ('entry', 'bundle_id', 'name', 'companion', 'icon', 'features'):
        if getattr(args, field) is None:
            setattr(args, field, metadata.get(field.replace('_', '-')))
    args.name = args.name or app['name']
    if args.icon:
        args.icon = app_manifest.parent / args.icon
        if not args.icon.is_file():
            parser.error(f'no icon at {args.icon}')
    args.features = args.features or ''
    args.deployment_target = args.deployment_target or ('26.0' if args.target == 'device' else '10.0')
    # Info.plist entries of the application's own, which go in last.
    extra_info = metadata.get('info-plist', {})
    if not isinstance(extra_info, dict):
        parser.error('package.metadata.cranpose.watchos.info-plist must be a table')
    if not args.entry or not args.bundle_id:
        parser.error('entry and bundle-id are required as flags or package.metadata.cranpose.watchos fields')
    if not re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*(::[A-Za-z_][A-Za-z_0-9]*)*', args.entry):
        parser.error('--entry must be a Rust function path')
    if not re.fullmatch(r'[A-Za-z0-9]+([.-][A-Za-z0-9]+)+', args.bundle_id):
        parser.error('invalid bundle identifier')
    if args.companion and not args.bundle_id.startswith(args.companion + '.'):
        parser.error('the watch bundle id must start with the companion bundle id and a dot')
    if not re.fullmatch(r'\d+(\.\d+){1,2}', args.deployment_target):
        parser.error('invalid deployment target')
    if args.target == 'device' and int(args.deployment_target.split('.')[0]) < 26:
        parser.error('arm64 watch devices require watchOS 26 or later')
    root = args.output.resolve()
    marker = root / '.cranpose-watchos-output'
    if root.exists() and any(root.iterdir()) and not marker.exists():
        parser.error('output directory is not an owned Cranpose watchOS build')
    root.mkdir(parents=True, exist_ok=True)
    marker.touch()
    runner = root / 'runner'
    source = args.framework_source.resolve() if args.framework_source else None
    patches = ''
    if source:
        workspace = tomllib.loads((source / 'Cargo.toml').read_text())['workspace']
        version = workspace['package']['version']
        patches = '\n[patch.crates-io]\n'
        for name, dependency in workspace['dependencies'].items():
            if isinstance(dependency, dict) and 'path' in dependency:
                # A patch names the package, which a renamed dependency
                # (`wgpu = { package = "cranpose-wgpu", ... }`) keeps apart
                # from its key.
                package = dependency.get('package', name)
                patches += f'{package} = {{ path = {json.dumps(str(source / dependency["path"]))} }}\n'
    else:
        version = '=' + (args.cranpose_version or tomllib.loads(
            (Path(__file__).resolve().parent.parent / 'Cargo.toml').read_text())['package']['version'])
    manifest = f'''[package]
name = "cranpose-watchos-runner"
version = "0.0.0"
edition = "2024"

[workspace]

[lib]
crate-type = ["staticlib", "rlib"]

[dependencies]
application = {{ package = {json.dumps(app['name'])}, path = {json.dumps(str(app_manifest.parent))}, default-features = false, features = {json.dumps([x for x in args.features.split(',') if x])} }}
cranpose = {{ version = {json.dumps(version)}, features = ["watchos"] }}

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
''' + patches
    write(runner / 'src/lib.rs', rust_bridge(args.entry, args.bundle_id))
    app_lock = next((parent / 'Cargo.lock' for parent in app_manifest.parents
                     if (parent / 'Cargo.lock').is_file()), None)
    prepare_manifest(parser, runner, manifest, app_lock)
    write(runner / 'build.rs', '''fn main() {
    cxx_build::bridge("src/lib.rs").std("c++17").compile("cranpose_watchos_bridge");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
''')
    templates = Path(__file__).resolve().parent
    write(runner / 'src/bin/snapshot.rs', (templates / 'Snapshot.rs').read_text())
    for file in ['Host.swift', 'Host.h', 'Host.mm']:
        shutil.copyfile(templates / file, root / file)
    if args.prepare_only:
        return
    if sys.platform != 'darwin':
        parser.error('Apple compilation requires macOS and the watchOS SDK')
    simulator = args.target == 'simulator'
    sdk_name = 'watchsimulator' if simulator else 'watchos'
    sdk = output(['xcrun', '--sdk', sdk_name, '--show-sdk-path'])
    target = 'aarch64-apple-watchos-sim' if simulator else 'aarch64-apple-watchos'
    apple_target = f'arm64-apple-watchos{args.deployment_target}' + ('-simulator' if simulator else '')
    capabilities = root / 'capabilities'
    env = dict(os.environ, SDKROOT=sdk, WATCHOS_DEPLOYMENT_TARGET=args.deployment_target,
               CARGO_TARGET_DIR=str(root / 'target'), CRANPOSE_CAPABILITIES_DIR=str(capabilities))
    import fcntl
    rustup_home = Path(env.get('RUSTUP_HOME', Path.home() / '.rustup'))
    rustup_home.mkdir(parents=True, exist_ok=True)
    with (rustup_home / 'cranpose-install.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        run(['rustup', 'target', 'add', target], cwd=runner, env=env)
    # The frameworks and system libraries the Rust code links, which the
    # static library cannot carry. Rustc writes them when it builds the
    # library, and the file stays for a build with nothing to compile.
    native_libs = root / 'native-static-libs.txt'
    # A published framework builds from the application's lock; a checkout's
    # dependencies move with it, so its build may add to the lock.
    locked = [] if source else ['--locked']
    run(['cargo', 'rustc', '--lib', '--release', *locked, '--target', target, '--',
         f'--print=native-static-libs={native_libs}'], cwd=runner, env=env)
    native_object = root / 'Host.o'
    run(['xcrun', '--sdk', sdk_name, 'clang++', '-target', apple_target,
         '-isysroot', sdk, '-std=c++17', '-fobjc-arc', '-O2', '-c', root / 'Host.mm',
         '-I', root / 'target/cxxbridge', '-o', native_object], env=env)
    bundle = root / 'CranposeWatch.app'
    bundle.mkdir(exist_ok=True)
    run(['xcrun', '--sdk', sdk_name, 'swiftc', '-target', apple_target,
         '-sdk', sdk, '-O', '-parse-as-library', '-import-objc-header', root / 'Host.h',
         root / 'Host.swift', native_object, root / f'target/{target}/release/libcranpose_watchos_runner.a',
         '-lc++', *native_libs.read_text().split(),
         '-o', bundle / 'CranposeWatch'], env=env)
    # A companion's version must match the iPhone app's, which an app keeps
    # equal to its package version.
    version = app.get('version')
    info = dict(CFBundleIdentifier=args.bundle_id, CFBundleName=args.name,
                CFBundleDisplayName=args.name, CFBundleExecutable='CranposeWatch',
                CFBundlePackageType='APPL', CFBundleVersion='1',
                CFBundleShortVersionString=version if isinstance(version, str) else '0.1',
                WKApplication=True,
                MinimumOSVersion=args.deployment_target, UIDeviceFamily=[4],
                CFBundleSupportedPlatforms=['WatchSimulator' if simulator else 'WatchOS'])
    if args.companion:
        info['WKCompanionAppBundleIdentifier'] = args.companion
    else:
        info['WKWatchOnly'] = True
    if args.icon:
        info.update(compile_icon(args.icon, root, bundle, sdk_name, args.deployment_target))
    # The reasons the application gave for its permissions, written by
    # `cranpose_capabilities::declare` in its build script.
    usage = capabilities / f"{app['name']}-usage.plist"
    if usage.is_file():
        info.update(plistlib.loads(usage.read_bytes()))
    info.update(extra_info)
    (bundle / 'Info.plist').write_bytes(plistlib.dumps(info))
    if simulator or args.sign:
        run(['codesign', '--force', '--sign', args.sign or '-', bundle])
    print(f'Built {args.target} app: {bundle}')


if __name__ == '__main__':
    main()
