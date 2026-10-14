import hashlib
import json
from pathlib import Path
import stat
import tempfile
import zipfile

ARCHIVE = 'Vega-macos-arm64.zip'
MAX_ARCHIVE = 512 * 1024 * 1024
MAX_EXPANDED = 2 * 1024 * 1024 * 1024
MAX_MANIFEST = 1024 * 1024


class UnsupportedBundleLayout(ValueError):
    pass


def digest(path):
    with open(path, 'rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def validate_paths(paths):
    folded = set()
    directories = {}
    for path in paths:
        if (not path or '\\' in path
                or any(part in ('', '.', '..') for part in path.split('/'))):
            raise ValueError('Invalid bundle path')
        if (not path.isascii() or len(path) > 1024 or not path.startswith('Contents/')
                or ':' in path or any(ord(char) < 32 or ord(char) == 127 for char in path)):
            raise UnsupportedBundleLayout('Unsupported bundle path')
        parts = path.split('/')
        for index in range(1, len(parts)):
            directory = '/'.join(parts[:index])
            previous = directories.setdefault(directory.lower(), directory)
            if previous != directory:
                raise ValueError('Case-aliased bundle directory')
        key = path.lower()
        if key in folded:
            raise ValueError('Duplicate bundle path')
        folded.add(key)
    for path in folded:
        parts = path.split('/')
        if any('/'.join(parts[:index]) in folded for index in range(1, len(parts))):
            raise ValueError('Bundle file/directory conflict')


def inventory(archive):
    result = {}
    total = 0
    with zipfile.ZipFile(archive) as source:
        for entry in source.infolist():
            if not entry.filename.startswith('Vega.app/'):
                continue
            path = entry.filename[len('Vega.app/'):]
            mode = entry.external_attr >> 16
            if entry.is_dir():
                if stat.S_IFMT(mode) != stat.S_IFDIR or stat.S_IMODE(mode) != 0o755:
                    raise UnsupportedBundleLayout('Unsupported bundle directory')
                if path.rstrip('/') != 'Contents':
                    validate_paths([path.rstrip('/')]) if path else None
                continue
            if stat.S_IFMT(mode) != stat.S_IFREG or stat.S_IMODE(mode) not in (0o644, 0o755):
                raise UnsupportedBundleLayout('Unsupported bundle file mode')
            if path in result or entry.flag_bits & 1:
                raise ValueError('Duplicate or encrypted bundle file')
            total += entry.file_size
            if total > MAX_EXPANDED or len(result) >= 4096:
                raise ValueError('Bundle exceeds size limits')
            with source.open(entry) as stream:
                sha256 = hashlib.file_digest(stream, 'sha256').hexdigest()
            result[path] = dict(path=path, size=entry.file_size, sha256=sha256,
                                mode=stat.S_IMODE(mode))
    validate_paths(result)
    if not {'Contents/MacOS/vega', 'Contents/Info.plist'} <= result.keys():
        raise ValueError('Bundle identity files missing')
    return result


def reconstruct(target, base_archive, patch_archive, changed):
    validate_paths(target)
    base = inventory(base_archive)
    with tempfile.TemporaryDirectory() as directory, \
            zipfile.ZipFile(base_archive) as old, zipfile.ZipFile(patch_archive) as patch:
        if [entry.filename for entry in patch.infolist()] != changed:
            raise ValueError('Patch inventory mismatch')
        root = Path(directory)
        for path, expected in target.items():
            source = patch if path in changed else old
            name = path if path in changed else 'Vega.app/' + path
            entry = source.getinfo(name)
            if path not in changed and base.get(path, {}).get('sha256') != expected['sha256']:
                raise ValueError('Reused file mismatch')
            if path in changed and entry.external_attr >> 16 != stat.S_IFREG | expected['mode']:
                raise ValueError('Patch mode mismatch')
            destination = root / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            with source.open(entry) as reader, destination.open('xb') as writer:
                while block := reader.read(64 * 1024):
                    writer.write(block)
            destination.chmod(expected['mode'])
            metadata = destination.stat()
            if (metadata.st_size != expected['size'] or digest(destination) != expected['sha256']
                    or stat.S_IMODE(metadata.st_mode) != expected['mode']):
                raise ValueError('Reconstructed file mismatch')
        actual = {path.relative_to(root).as_posix() for path in root.rglob('*') if path.is_file()}
        if actual != target.keys():
            raise ValueError('Reconstructed inventory mismatch')


def generate(dist, version, bases):
    dist = Path(dist)
    full = dist / ARCHIVE
    try:
        target = inventory(full)
    except UnsupportedBundleLayout as error:
        print(f'Publishing full-only delta manifest: {error}')
        target = {}
    patches = []
    for from_version, base_archive in (bases[:5] if target else []):
        base = inventory(base_archive)
        changed = sorted(path for path, record in target.items()
                         if any(base.get(path, {}).get(key) != record[key]
                                for key in ('sha256', 'size')))
        name = f'Vega-delta-{from_version}-to-{version}.zip'
        output = dist / name
        with zipfile.ZipFile(full) as source, zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as patch:
            for path in changed:
                entry = zipfile.ZipInfo(path)
                entry.create_system = 3
                entry.external_attr = (stat.S_IFREG | target[path]['mode']) << 16
                entry.compress_type = zipfile.ZIP_DEFLATED
                with source.open('Vega.app/' + path) as reader, patch.open(entry, 'w') as writer:
                    while block := reader.read(64 * 1024):
                        writer.write(block)
        if output.stat().st_size >= min(full.stat().st_size, MAX_ARCHIVE + 1):
            output.unlink()
            continue
        reconstruct(target, base_archive, output, changed)
        patches.append(dict(from_version=from_version, archive=name, size=output.stat().st_size,
                            sha256=digest(output), files=changed))
    manifest = dict(schema=1, version=version, bundle_id='ai.vega', target='aarch64-apple-darwin',
                    full_sha256=digest(full), files=[target[path] for path in sorted(target)], patches=patches)
    raw = json.dumps(manifest, separators=(',', ':')).encode()
    if len(raw) > MAX_MANIFEST:
        raise ValueError('Delta manifest exceeds size limit')
    (dist / 'Vega-delta.json').write_bytes(raw)
    return manifest
