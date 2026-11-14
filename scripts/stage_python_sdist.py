"""Retain the complete locked workspace in a raw maturin 1.9.6 source archive."""
from pathlib import Path, PurePosixPath
import argparse
import gzip
import hashlib
import io
import tarfile
import tempfile
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


def check_member_sources(prefix, files, member, root=ROOT):
    folder = root / member
    source_manifest = (folder / 'Cargo.toml').read_bytes()
    archived_manifest = files[prefix + '/' + member + '/Cargo.toml'][0]
    relocated = source_manifest.replace(b'readme.workspace = true\n', b'readme = "README.md"\n')
    assert archived_manifest in (source_manifest, relocated), 'Changed Cargo manifest: ' + member
    if archived_manifest != source_manifest:
        assert files[prefix + '/' + member + '/README.md'][0] == (root / 'README.md').read_bytes()
    sources = list((folder / 'src').rglob('*.rs'))
    assert sources, 'Missing Rust sources: ' + member
    expected = {prefix + '/' + path.relative_to(root).as_posix() for path in sources}
    actual = {name for name in files if name.startswith(prefix + '/' + member + '/src/') and name.endswith('.rs')}
    assert actual == expected, 'Incomplete or extra Rust sources: ' + member
    for name in ('build.rs', 'LICENSE'):
        if (folder / name).is_file():
            sources.append(folder / name)
    for path in sources:
        name = prefix + '/' + path.relative_to(root).as_posix()
        assert name in files and files[name][0] == path.read_bytes(), 'Missing or changed source: ' + name


def stage(source, destination):
    source, destination = source.resolve(), destination.resolve()
    assert source != destination and not destination.exists(), 'Refusing to overwrite a source archive'
    prefix, files = read_archive(source)
    project = tomllib.loads((ROOT / 'python/pyproject.toml').read_text())['project']
    assert prefix == project['name'] + '-' + project['version'], 'Unexpected source package'
    assert prefix + '/pyproject.toml' in files and prefix + '/PKG-INFO' in files
    lock = (ROOT / 'Cargo.lock').read_bytes()
    assert files[prefix + '/Cargo.lock'][0] == lock, 'Raw source lock differs from the adopted workspace'
    replacement = {ROOT / name for name in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/config.toml']}
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
        check_member_sources(prefix, files, member)
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


def self_test():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        folder = root / 'crates/example'
        (folder / 'src').mkdir(parents=True)
        (root / 'README.md').write_text('public readme')
        for name, data in [('Cargo.toml', '[package]\nname="example"\nreadme.workspace = true\n'),
                           ('src/lib.rs', 'pub fn example() {}\n'), ('build.rs', 'fn main() {}\n'),
                           ('LICENSE', 'public license')]:
            (folder / name).write_text(data)
        files = {'example/' + p.relative_to(root).as_posix(): (p.read_bytes(), 0o644)
                 for p in folder.rglob('*') if p.is_file()}
        check_member_sources('example', files, 'crates/example', root)
        for name in list(files):
            missing = dict(files); del missing[name]
            changed = dict(files); changed[name] = (b'altered', 0o644)
            for invalid in (missing, changed):
                try:
                    check_member_sources('example', invalid, 'crates/example', root)
                except (AssertionError, KeyError):
                    pass
                else:
                    raise AssertionError('Source guard accepted missing/changed ' + name)
    print('Source archive completeness/byte guards passed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('raw_archive', type=Path, nargs='?')
    parser.add_argument('final_archive', type=Path, nargs='?')
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        self_test()
    else:
        if args.raw_archive is None or args.final_archive is None:
            parser.error('raw_archive and final_archive are required')
        stage(args.raw_archive, args.final_archive)
