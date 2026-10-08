#!/usr/bin/env python3
"""Which version of each framework a gauntlet run measures, where each app's
gauntlet source is, and moving each framework to its latest stable release.

  versions.py check                 the pinned and the latest version of each
  versions.py bump --record FILE    moves every pin behind its latest release
                                    forward; FILE lists what moved
  versions.py revert PIN --record FILE
                                    puts the pin back, as a build of the
                                    moved pin failed
  versions.py write FILE --platform android|desktop
                                    the version every app was built with, as
                                    the nightly hands it to the phone's job

A framework without a pin of ours follows its platform instead: the WebView
or Chrome the device has, the macOS SwiftUI ships with, and the Flutter SDK
and .NET MAUI workload `build_apps.sh` keeps current.
"""

import argparse
import json
import re
import subprocess
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

# The file each app draws the gauntlet in, from the repository's root.
SOURCES = {
    'cranpose': 'benchmarks/compose-vs-cranpose/cranpose-app/src/screens/gauntlet.rs',
    'cranpose-release': 'benchmarks/compose-vs-cranpose/cranpose-app/src/screens/gauntlet.rs',
    'compose': 'benchmarks/compose-vs-cranpose/shared-compose/dev/perfcompare/compose/Gauntlet.kt',
    'views': 'benchmarks/compose-vs-cranpose/views-app/app/src/main/java/dev/perfcompare/views/Gauntlet.kt',
    'flutter': 'benchmarks/compose-vs-cranpose/flutter-app/lib/main.dart',
    'rn': 'benchmarks/compose-vs-cranpose/rn-app/src/Gauntlet.tsx',
    'maui': 'benchmarks/compose-vs-cranpose/maui-app/GauntletPage.cs',
    'avalonia': 'benchmarks/compose-vs-cranpose/avalonia-app/Gauntlet.cs',
    'egui': 'benchmarks/compose-vs-cranpose/egui-app/src/lib.rs',
    'slint': 'benchmarks/compose-vs-cranpose/slint-app/ui/gauntlet.slint',
    'iced': 'benchmarks/compose-vs-cranpose/iced-app/src/main.rs',
    'gpui': 'benchmarks/compose-vs-cranpose/gpui-app/src/main.rs',
    'swiftui': 'benchmarks/compose-vs-cranpose/swiftui-app/Gauntlet.swift',
    'appkit': 'benchmarks/compose-vs-cranpose/appkit-app/Gauntlet.swift',
    'web': 'benchmarks/compose-vs-cranpose/web-app/src/gauntlet.ts',
    'tauri': 'benchmarks/compose-vs-cranpose/web-app/src/gauntlet.ts',
    'dioxus': 'benchmarks/compose-vs-cranpose/dioxus-app/src/main.rs',
    'xilem': 'benchmarks/compose-vs-cranpose/xilem-app/src/main.rs',
    'fyne': 'benchmarks/compose-vs-cranpose/fyne-app/main.go',
    'nativescript': 'benchmarks/compose-vs-cranpose/nativescript-app/app/app.ts',
    'lynx': 'benchmarks/compose-vs-cranpose/lynx-app/page/src/Gauntlet.tsx',
}

# The apps each platform runs besides Cranpose's.
PLATFORM_APPS = {
    'android': ['compose', 'views', 'flutter', 'rn', 'nativescript', 'lynx', 'maui', 'avalonia', 'egui',
                'slint', 'web'],
    'desktop': ['compose', 'egui', 'slint', 'iced', 'gpui', 'avalonia', 'swiftui', 'appkit', 'flutter', 'web',
                'tauri', 'dioxus', 'xilem', 'fyne'],
}

NAMES = {
    'cranpose': 'Cranpose', 'cranpose-release': 'Cranpose', 'compose': 'Compose', 'views': 'Views',
    'flutter': 'Flutter', 'rn': 'React Native', 'maui': '.NET MAUI', 'avalonia': 'Avalonia',
    'egui': 'egui', 'slint': 'Slint', 'iced': 'iced', 'gpui': 'GPUI', 'swiftui': 'SwiftUI', 'appkit': 'AppKit',
    'web': 'Web', 'tauri': 'Tauri', 'dioxus': 'Dioxus', 'xilem': 'Xilem',
    'fyne': 'Fyne', 'nativescript': 'NativeScript', 'lynx': 'Lynx',
}


def read(path):
    return (HERE / path).read_text()


def run(*command, cwd=HERE):
    return subprocess.run(command, cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def locked(app_dir, crate):
    """The version of `crate` the app's Cargo.lock holds."""
    match = re.search(rf'\[\[package\]\]\nname = "{re.escape(crate)}"\nversion = "([^"]+)"', read(f'{app_dir}/Cargo.lock'))
    return match.group(1) if match else '?'


def fetch(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'cranpose-perf-gauntlet'})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read().decode()


def stable(version):
    return not re.search(r'[-+]|alpha|beta|rc|dev', version, re.IGNORECASE)


def numbers(version):
    """The version's numbers, padded so `0.14` and `0.14.0` compare equal."""
    parts = [int(part) for part in re.findall(r'\d+', version)]
    return tuple(parts + [0] * (4 - len(parts)))


def newest(versions):
    candidates = [version for version in versions if stable(version)]
    return max(candidates, key=numbers) if candidates else None


def crates_io(crate):
    return json.loads(fetch(f'https://crates.io/api/v1/crates/{crate}'))['crate']['max_stable_version']


def nuget(package):
    return newest(json.loads(fetch(f'https://api.nuget.org/v3-flatcontainer/{package.lower()}/index.json'))['versions'])


def maven(base, group, artifact):
    metadata = fetch(f'{base}/{group.replace(".", "/")}/{artifact}/maven-metadata.xml')
    return newest(re.findall(r'<version>([^<]+)</version>', metadata))


def npm(package):
    return json.loads(fetch(f'https://registry.npmjs.org/{package}/latest'))['version']


GOOGLE = 'https://dl.google.com/android/maven2'
CENTRAL = 'https://repo1.maven.org/maven2'


class Pin:
    """A framework version the repository pins: where, what it reads now,
    what its registry offers, and how a newer one is put in."""

    def __init__(self, files, pattern, latest, refresh=None):
        self.files = files
        self.pattern = pattern
        self.latest = latest
        self.refresh = refresh

    def current(self):
        return re.search(self.pattern, read(self.files[0])).group(1)

    def set(self, version):
        for file in self.files:
            path = HERE / file
            path.write_text(re.sub(self.pattern, lambda match: match.group(0).replace(match.group(1), version),
                                   path.read_text()))
        if self.refresh:
            self.refresh()


def cargo_update(app_dir):
    return lambda: run('cargo', 'update', cwd=HERE / app_dir)


def egui_update():
    """`cargo update`, then the AccessKit adapter and winit eframe's tree
    uses: a second copy beside ours would take the Android adapter's feature
    with it, and nothing would fail."""
    cargo_update('egui-app')()
    manifest = HERE / 'egui-app/Cargo.toml'
    for crate in ('accesskit_winit', 'winit'):
        lock = read('egui-app/Cargo.lock')
        versions = re.findall(rf'\[\[package\]\]\nname = "{crate}"\nversion = "([^"]+)"', lock)
        if len(versions) > 1:
            text = manifest.read_text()
            manifest.write_text(re.sub(rf'({crate} = \{{ version = ")[^"]+"', rf'\g<1>{max(versions, key=numbers)}"', text))
            cargo_update('egui-app')()


def go_module(module):
    """The latest release the Go module proxy knows of `module`."""
    return json.loads(fetch(f'https://proxy.golang.org/{module}/@latest'))['Version'].lstrip('v')


def go_tidy(app_dir):
    return lambda: run('go', 'mod', 'tidy', cwd=HERE / app_dir)


def npm_lock(app_dir):
    return lambda: run('npm', 'install', '--package-lock-only', '--no-audit', '--no-fund', cwd=HERE / app_dir)


PINS = {
    'egui': Pin(['egui-app/Cargo.toml'], r'eframe = \{ version = "([^"]+)"', lambda: crates_io('eframe'),
                egui_update),
    'slint': Pin(['slint-app/Cargo.toml'], r'slint(?:-build)? = (?:\{ version = )?"([^"]+)"',
                 lambda: crates_io('slint'), cargo_update('slint-app')),
    'iced': Pin(['iced-app/Cargo.toml'], r'iced = \{ version = "([^"]+)"', lambda: crates_io('iced'),
                cargo_update('iced-app')),
    'gpui': Pin(['gpui-app/Cargo.toml'], r'package = "gpui-pre(?:-platform)?", version = "=([^"]+)"',
                lambda: crates_io('gpui-pre'), cargo_update('gpui-app')),
    'avalonia': Pin(['avalonia-app/PerfAvalonia.csproj'], r'Include="Avalonia[.\w]*" Version="([^"]+)"',
                    lambda: nuget('Avalonia')),
    'compose': Pin(['compose-app/app/build.gradle.kts'], r'compose-bom:([\d.]+)',
                   lambda: maven(GOOGLE, 'androidx.compose', 'compose-bom')),
    'compose-desktop': Pin(['compose-desktop-app/settings.gradle.kts'], r'id\("org.jetbrains.compose"\) version "([^"]+)"',
                           lambda: maven(CENTRAL, 'org.jetbrains.compose', 'org.jetbrains.compose.gradle.plugin')),
    'views': Pin(['views-app/app/build.gradle.kts'], r'androidx.recyclerview:recyclerview:([\d.]+)',
                 lambda: maven(GOOGLE, 'androidx.recyclerview', 'recyclerview')),
    'rn': Pin(['rn-app/package.json'], r'"react-native": "([^"]+)"', lambda: npm('react-native'), npm_lock('rn-app')),
    'lynx': Pin(['lynx-app/app/build.gradle.kts'], r'val lynx = "([^"]+)"',
                lambda: maven(CENTRAL, 'org.lynxsdk.lynx', 'lynx')),
    'nativescript': Pin(['nativescript-app/package.json'], r'"@nativescript/(?:core|types)": "([^"]+)"',
                        lambda: npm('@nativescript/core'), npm_lock('nativescript-app')),
    'web': Pin(['web-app/package.json'], r'"@capacitor/(?:core|android|cli)": "([^"]+)"',
               lambda: npm('@capacitor/core'), npm_lock('web-app')),
    'tauri': Pin(['tauri-app/Cargo.toml'], r'tauri = \{ version = "([^"]+)"', lambda: crates_io('tauri'),
                 cargo_update('tauri-app')),
    'dioxus': Pin(['dioxus-app/Cargo.toml'], r'dioxus = \{ version = "([^"]+)"', lambda: crates_io('dioxus'),
                  cargo_update('dioxus-app')),
    'xilem': Pin(['xilem-app/Cargo.toml'], r'xilem = \{ version = "([^"]+)"', lambda: crates_io('xilem'),
                 cargo_update('xilem-app')),
    'fyne': Pin(['fyne-app/go.mod'], r'fyne\.io/fyne/v2 v([\d.]+)', lambda: go_module('fyne.io/fyne/v2'),
                go_tidy('fyne-app')),
}

# Each Rust app's folder and the crate whose locked version names its
# framework.
RUST_CRATES = {
    'egui': ('egui-app', 'eframe'), 'slint': ('slint-app', 'slint'), 'iced': ('iced-app', 'iced'),
    'gpui': ('gpui-app', 'gpui-pre'), 'tauri': ('tauri-app', 'tauri'), 'dioxus': ('dioxus-app', 'dioxus'),
    'xilem': ('xilem-app', 'xilem'),
}
# Apps that draw in the system's WKWebView on the desktop.
WEBKIT = {'tauri', 'dioxus'}

def chrome_version():
    plist = Path('/Applications/Google Chrome.app/Contents/Info.plist')
    return run('defaults', 'read', str(plist.with_suffix('')), 'CFBundleShortVersionString')


def safari_version():
    """The WebKit of the system: Safari's version."""
    return run('defaults', 'read', '/Applications/Safari.app/Contents/Info', 'CFBundleShortVersionString')


def flutter_version():
    return json.loads(run('flutter', '--version', '--machine'))['frameworkVersion']


def maui_version():
    """The MAUI the installed workload builds the app with: the project's
    `MauiVersion`, which MSBuild reads without restoring, so a build the cache
    kept names it too."""
    return run('dotnet', 'msbuild', '-getProperty:MauiVersion', cwd=HERE / 'maui-app')


def cranpose_version(platform, release=None):
    if release:
        return release
    crate = re.search(r'\[workspace\.package\]\nversion = "([^"]+)"', (REPO / 'Cargo.toml').read_text()).group(1)
    return f'{crate} main {run("git", "rev-parse", "--short=9", "HEAD")}'


def version(app, platform, release=None):
    """The version of `app`'s framework this checkout builds, on `platform`."""
    if app == 'cranpose':
        return cranpose_version(platform)
    if app == 'cranpose-release':
        return cranpose_version(platform, release)
    if app == 'compose':
        return (f'BOM {PINS["compose"].current()}' if platform == 'android'
                else f'Multiplatform {PINS["compose-desktop"].current()}')
    if app in RUST_CRATES:
        found = locked(*RUST_CRATES[app])
        return f'{found}, WKWebView {safari_version()}' if app in WEBKIT and platform == 'desktop' else found
    if app == 'flutter':
        return flutter_version()
    if app == 'maui':
        return maui_version()
    if app in ('swiftui', 'appkit'):
        return f'macOS {run("sw_vers", "-productVersion")}'
    if app == 'web':
        capacitor = PINS['web'].current()
        return f'Chrome {chrome_version()}' if platform == 'desktop' else f'Capacitor {capacitor}'
    return PINS[app].current()


def subject(app, platform, versions=None, release=None):
    """A run's subject: the app, its framework and version, and its source.
    A version no file or tool on this machine names reads as unknown, so a
    night's measurements are kept."""
    found = (versions or {}).get(app)
    if not found:
        try:
            found = version(app, platform, release)
        except (OSError, subprocess.CalledProcessError) as error:
            print(f'{app}: no version: {error}', flush=True)
            found = 'unknown'
    return {'name': app, 'label': f'{NAMES[app]} {found}', 'version': found, 'source': SOURCES[app]}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=['check', 'bump', 'revert', 'write'])
    parser.add_argument('target', nargs='?', help='the pin to revert, or the file to write')
    parser.add_argument('--record', type=Path)
    parser.add_argument('--platform', choices=['android', 'desktop'])
    args = parser.parse_args()

    if args.command == 'write':
        apps = PLATFORM_APPS[args.platform]
        Path(args.target).write_text(json.dumps({app: version(app, args.platform) for app in apps}, indent=1))
        return

    record = json.loads(args.record.read_text()) if args.record and args.record.exists() else {}
    if args.command == 'revert':
        moved = record[args.target]
        PINS[args.target].set(moved['from'])
        moved['reverted'] = True
        args.record.write_text(json.dumps(record, indent=1))
        print(f'{args.target}: back to {moved["from"]}, as {moved["to"]} did not build')
        return

    for name, pin in PINS.items():
        current = pin.current()
        try:
            latest = pin.latest()
        except Exception as error:  # a registry that does not answer leaves the pin alone
            print(f'{name:16} {current:>12}  latest unknown: {error}')
            continue
        behind = latest and numbers(latest) > numbers(current)
        print(f'{name:16} {current:>12}  latest {latest}{"  <- behind" if behind else ""}')
        if args.command == 'bump' and behind:
            pin.set(latest)
            record[name] = {'from': current, 'to': latest, 'files': pin.files, 'reverted': False}
    if args.record:
        args.record.write_text(json.dumps(record, indent=1))


if __name__ == '__main__':
    main()
