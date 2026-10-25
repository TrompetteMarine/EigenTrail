"""Retain the complete locked workspace in a raw maturin 1.9.6 source archive."""
from pathlib import Path, PurePosixPath
import argparse
import gzip
import hashlib
import io
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def read_archive(path):
    files, names, roots = {}, set(), set()
    with tarfile.open(path) as archive:
        members = archive.getmembers()
        assert sum(member.size for member in members) <= 256 * 1024**2, 'Oversized source archive'
        for member in members:
            name = PurePosixPath(member.name)
            assert not name.is_absolute() and '..' not in name.parts and '\\' not in member.name, 'Unsafe archive name'
            assert name.as_posix() == member.name.rstrip('/'), 'Noncanonical archive name'
            assert name.as_posix() not in names, 'Duplicate archive member'
            names.add(name.as_posix())
            assert member.isfile() or member.isdir(), 'Links/devices are not source files'
            roots.add(name.parts[0])
            if member.isfile():
                assert len(name.parts) > 1, 'File outside source root'
                files[name.as_posix()] = (archive.extractfile(member).read(), member.mode & 0o777)
    assert len(roots) == 1 and files, 'Ambiguous source root'
    return roots.pop(), files


def stage(source, destination):
    source, destination = source.resolve(), destination.resolve()
    assert source != destination and not destination.exists(), 'Refusing to overwrite a source archive'
    prefix, files = read_archive(source)
    project = tomllib.loads((ROOT / 'python/pyproject.toml').read_text())['project']
    assert prefix == project['name'] + '-' + project['version'], 'Unexpected source package'
    assert prefix + '/pyproject.toml' in files and prefix + '/PKG-INFO' in files
    lock = (ROOT / 'Cargo.lock').read_bytes()
    assert files[prefix + '/Cargo.lock'][0] == lock, 'Raw source lock differs from the adopted workspace'
    replacement = {ROOT / name for name in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']}
    replacement.update(path for path in (ROOT / 'crates/eigentrail-r').rglob('*') if path.is_file())
    changed = set()
    for path in sorted(replacement):
        assert not path.is_symlink(), 'Linked source input'
        name = prefix + '/' + path.relative_to(ROOT).as_posix()
        files[name] = (path.read_bytes(), path.stat().st_mode & 0o777)
        changed.add(name)
    workspace = tomllib.loads(files[prefix + '/Cargo.toml'][0].decode())['workspace']
    assert workspace['members'] == tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['members']
    for member in workspace['members']:
        assert prefix + '/' + member + '/Cargo.toml' in files, 'Incomplete workspace member: ' + member
    with destination.open('xb') as output, gzip.GzipFile(filename='', fileobj=output, mode='wb', mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w') as archive:
            for name, (data, mode) in sorted(files.items()):
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = len(data), mode, 0
                archive.addfile(info, io.BytesIO(data))
    result_prefix, result = read_archive(destination)
    assert result_prefix == prefix and result == files, 'Source archive round trip changed bytes'
    assert result[prefix + '/Cargo.lock'][0] == lock
    _, original = read_archive(source)
    assert all(result[name] == content for name, content in original.items() if name not in changed)
    assert (ROOT / 'Cargo.lock').read_bytes() == lock, 'Root lock changed during staging'
    print('Preserved raw maturin metadata and source bytes; retained all', len(workspace['members']), 'locked workspace members.')
    print(destination, 'sha256', hashlib.sha256(destination.read_bytes()).hexdigest())


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('raw_archive', type=Path)
    parser.add_argument('final_archive', type=Path)
    args = parser.parse_args()
    stage(args.raw_archive, args.final_archive)
