import importlib.util
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('watchos_build', Path(__file__).parents[1] / 'build.py')
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class PackagerTest(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.manifest = self.root / 'app/Cargo.toml'
        self.manifest.parent.mkdir()
        self.original = '[package]\nname="test-app"\nversion="1.0.0"\n'
        self.manifest.write_text(self.original)
        self.destination = self.root / 'output'
        self.args = ['build.py', '--manifest', str(self.manifest), '--entry', 'ui::Root',
                     '--bundle-id', 'org.cranpose.test', '--name', 'Test',
                     '--output', str(self.destination), '--cranpose-version', '0.1.177', '--prepare-only']

    def invoke(self):
        with patch.object(sys, 'argv', self.args), patch.object(build, 'run') as commands:
            build.main()
            return commands

    def test_preparation_preserves_application_and_owns_all_native_sources(self):
        self.invoke()
        self.assertEqual(self.manifest.read_text(), self.original)
        self.assertEqual(list(self.manifest.parent.iterdir()), [self.manifest])
        for name in ('Host.swift', 'Host.mm', 'Host.h', 'runner/src/lib.rs', 'runner/build.rs'):
            self.assertTrue((self.destination / name).is_file(), name)

    def test_unrelated_output_is_never_overwritten(self):
        self.destination.mkdir()
        saved = self.destination / 'important.txt'
        saved.write_text('preserve')
        with self.assertRaises(SystemExit):
            self.invoke()
        self.assertEqual(saved.read_text(), 'preserve')
        self.assertEqual(list(self.destination.iterdir()), [saved])

    def test_invalid_rust_entry_is_rejected_before_writing(self):
        self.args[self.args.index('--entry') + 1] = 'ui::Root); panic!("injected")'
        with self.assertRaises(SystemExit):
            self.invoke()
        self.assertFalse(self.destination.exists())

    def test_metadata_supplies_app_identity_without_native_sources(self):
        self.manifest.write_text(self.original + '''
[package.metadata.cranpose.watchos]
entry = "ui::Root"
bundle-id = "org.cranpose.metadata"
name = "Metadata App"
''')
        for option in ('--entry', '--bundle-id', '--name'):
            offset = self.args.index(option)
            del self.args[offset:offset + 2]
        self.invoke()
        bridge = (self.destination / 'runner/src/lib.rs').read_text()
        self.assertIn('application::ui::Root', bridge)
        self.assertIn('org.cranpose.metadata', bridge)

    def test_configuration_upgrade_keeps_output_usable(self):
        self.test_preparation_recovers_after_interrupted_dependency_setup()
        self.args[self.args.index('--cranpose-version') + 1] = '0.9.1'
        commands = self.invoke()
        self.assertEqual(commands.call_args.args[0], ['cargo', 'update', '--workspace'])
        self.assertIn('=0.9.1', (self.destination / 'runner/Cargo.toml').read_text())

    def test_framework_source_patches_each_path_dependency_by_package_name(self):
        framework = self.root / 'framework'
        framework.mkdir()
        (framework / 'Cargo.toml').write_text('''[workspace.package]
version = "0.9.4"

[workspace.dependencies]
cranpose-core = { path = "crates/cranpose-core", version = "0.9.4" }
wgpu = { package = "cranpose-wgpu", path = "forks/wgpu/wgpu", version = "30.0.1" }
log = "0.4"
''')
        offset = self.args.index('--cranpose-version')
        self.args[offset:offset + 2] = ['--framework-source', str(framework)]
        self.invoke()
        runner = tomllib.loads((self.destination / 'runner/Cargo.toml').read_text())
        source = framework.resolve()
        self.assertEqual(runner['patch']['crates-io'], {
            'cranpose-core': {'path': str(source / 'crates/cranpose-core')},
            'cranpose-wgpu': {'path': str(source / 'forks/wgpu/wgpu')},
        })

    def test_device_rejects_unsupported_deployment_target(self):
        self.args.extend(['--target', 'device', '--deployment-target', '10.0'])
        with self.assertRaises(SystemExit):
            self.invoke()
        self.assertFalse(self.destination.exists())

    def test_runner_inherits_application_lock_without_modifying_it(self):
        lock = self.manifest.parent / 'Cargo.lock'
        lock.write_text('# the application dependency resolution\nversion = 4\n')
        self.invoke()
        self.assertEqual(lock.read_text(), (self.destination / 'runner/Cargo.lock').read_text())

    def test_preparation_recovers_after_interrupted_dependency_setup(self):
        self.invoke()
        manifest = self.destination / 'runner/Cargo.toml'
        manifest.write_text(manifest.read_text().replace('[dependencies]', '[dependencies]\ncxx = "1"'))
        commands = self.invoke()
        self.assertEqual(commands.call_count, 1)
        self.assertEqual(commands.call_args.args[0], ['cargo', 'add', 'cxx-build@1', '--build'])
        manifest.write_text(manifest.read_text() + '\n[build-dependencies]\ncxx-build = "1"\n')
        before = manifest.read_text()
        self.assertEqual(self.invoke().call_count, 0)
        self.assertEqual(manifest.read_text(), before)


if __name__ == '__main__':
    unittest.main()
