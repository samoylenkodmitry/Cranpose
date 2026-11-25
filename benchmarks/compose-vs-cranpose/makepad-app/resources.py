"""Install Makepad's live-design resources under the macOS bundle resource path."""
import json
import shutil
import subprocess
import sys
from pathlib import Path


def install(destination):
    here = Path(__file__).resolve().parent
    metadata = subprocess.check_output(['cargo', 'metadata', '--locked', '--filter-platform', 'aarch64-apple-darwin', '--format-version', '1'], cwd=here)
    for package in json.loads(metadata)['packages']:
        if package['name'].startswith('makepad-'):
            resources = Path(package['manifest_path']).parent / 'resources'
            if resources.is_dir():
                crate = package['name'].replace('-', '_')
                shutil.copytree(resources, destination / crate / 'resources')
    fonts = destination / 'fonts'
    fonts.mkdir(parents=True)
    for file in ('Roboto-Regular.ttf', 'Roboto-Bold.ttf', 'DejaVuSansMono.ttf'):
        shutil.copy2(here.parent / 'fonts' / file, fonts)


if __name__ == '__main__':
    install(Path(sys.argv[1]))
