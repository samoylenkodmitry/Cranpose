import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import tarfile
import tempfile
import zipfile

from android_benchmark_support import digest


CACHE_SIGNATURE = 'Signature: 8a477f597d28d172789f06886806bc55'
INVENTORY = '.cranpose-source.json'


def source_inventory(root):
    root = Path(root)
    result = {}
    for directory, children, files in os.walk(root):
        base = Path(directory)
        children[:] = sorted(child for child in children
                             if child not in {'.git', '.gradle', 'target', 'build', '__pycache__'}
                             and not (base / child / 'CACHEDIR.TAG').exists())
        for name in sorted(children + files):
            if (base / name).is_symlink():
                raise ValueError('Source symlink requires an explicit materialized input: ' + str(base / name))
        for name in sorted(files):
            path = base / name
            if name in {INVENTORY, '.git'}:
                continue
            result[path.relative_to(root).as_posix()] = {
                'sha256': digest(path), 'executable': bool(path.stat().st_mode & 0o111),
            }
    return dict(sorted(result.items()))


def snapshot_source(root, destination):
    root, destination = Path(root), Path(destination)
    if destination.resolve().is_relative_to(root.resolve()):
        raise ValueError('Source archives must live outside their input tree')
    inventory = source_inventory(root)
    with destination.open('xb') as output, gzip.GzipFile(fileobj=output, mode='wb', mtime=0, filename='') as compressed:
        with tarfile.open(fileobj=compressed, mode='w|') as archive:
            for name, entry in [*inventory.items(), (INVENTORY, {'executable': False})]:
                data = (json.dumps(inventory, sort_keys=True).encode() if name == INVENTORY
                        else (root / name).read_bytes())
                if name != INVENTORY and hashlib.sha256(data).hexdigest() != entry['sha256']:
                    raise ValueError('Source changed while archiving: ' + name)
                member = tarfile.TarInfo(name)
                member.size = len(data)
                member.mode = 0o755 if entry['executable'] else 0o644
                archive.addfile(member, io.BytesIO(data))
    if source_inventory(root) != inventory:
        raise ValueError('Source inventory changed while archiving')
    destination.chmod(0o444)
    return digest(destination)


def extract_source(archive_path, destination):
    destination = Path(destination)
    if destination.exists():
        raise ValueError('Source extraction requires a fresh directory')
    with tarfile.open(archive_path) as archive:
        members = archive.getmembers()
        names = [member.name for member in members]
        if len(set(names)) != len(names):
            raise ValueError('Archive contains duplicate entries')
        for member in members:
            path = PurePosixPath(member.name)
            if not member.isfile() or path.is_absolute() or '..' in path.parts or str(path) != member.name:
                raise ValueError('Invalid source archive entry: ' + member.name)
        manifest = json.load(archive.extractfile(INVENTORY))
        if set(names) != set(manifest) | {INVENTORY}:
            raise ValueError('Archive inventory does not match its members')
        for member in members:
            if member.name != INVENTORY:
                actual = hashlib.file_digest(archive.extractfile(member), 'sha256').hexdigest()
                if actual != manifest[member.name]['sha256']:
                    raise ValueError('Archive source hash mismatch: ' + member.name)
        destination.mkdir(parents=True)
        for member in members:
            path = destination / member.name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(archive.extractfile(member).read())
            executable = member.name != INVENTORY and manifest[member.name]['executable']
            path.chmod(0o755 if executable else 0o644)
    if source_inventory(destination) != manifest:
        raise ValueError('Extracted source inventory mismatch')
    return manifest


def build_cache(cache_root):
    root = Path(cache_root).expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix='android-', dir=root))
    (directory / 'CACHEDIR.TAG').write_text(CACHE_SIGNATURE + '\n')
    return directory


def verify_apk(apk, proof, native_member):
    if digest(apk) != proof['apk_sha256']:
        raise ValueError('APK differs from its build provenance')
    with zipfile.ZipFile(apk) as archive:
        if len(archive.namelist()) != len(set(archive.namelist())):
            raise ValueError('APK contains duplicate members')
        native_hash = hashlib.sha256(archive.read(native_member)).hexdigest()
        if native_hash != proof['build']['native_sha256']:
            raise ValueError('APK native library differs from its source build')
        libraries = [name for name in archive.namelist()
                     if name.startswith('lib/') and name.endswith('/' + Path(native_member).name)]
        if libraries != [native_member]:
            raise ValueError('APK contains another ABI of the measured library')
        payload = apk_payload(archive, native_member)
    if payload != proof['payload']:
        raise ValueError('APK application payload differs from its provenance')
    return native_hash


def apk_payload(archive, native_member):
    return {name: hashlib.sha256(archive.read(name)).hexdigest()
            for name in archive.namelist()
            if not name.startswith('META-INF/')
            and not (name.startswith('lib/') and name.endswith('/' + Path(native_member).name))}


def verify_build(proof, directory):
    if proof['status'] != 'complete':
        raise ValueError('Native build did not complete')
    if digest(directory / proof['native']) != proof['native_sha256']:
        raise ValueError('Native artifact differs from its build report')
    for source in proof['sources'].values():
        archive = directory / source['archive']
        if digest(archive) != source['sha256']:
            raise ValueError('Source archive differs from its build report')
        with tarfile.open(archive) as contents:
            if json.load(contents.extractfile(INVENTORY)) != source['inventory']:
                raise ValueError('Source inventory differs from its build report')
