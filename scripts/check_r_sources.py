"""Check that the installable R package contains the current Rust sources."""
from pathlib import Path, PurePosixPath
import hashlib
import json
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def production_manifest(text):
    """Project Dev metadata into the same production manifest shipped publicly."""
    lines, dropping = [], False
    for line in text.splitlines(keepends=True):
        if line.lstrip().startswith('['):
            table = line.strip().strip('[]').strip()
            dropping = table == 'dev-dependencies' or table.startswith('dev-dependencies.') or '.dev-dependencies' in table
        if not dropping:
            lines.append(line)
    result = ''.join(lines).rstrip() + '\n'
    assert 'dev-dependencies' not in tomllib.loads(result)
    return result


def public_readme(root):
    overlay = root / 'release/public/README.md'
    return (overlay if overlay.is_file() else root / 'README.md').read_bytes()


def backport_sources(folder):
    """Select only hash-bound provenance inputs, without following links."""
    provenance = folder / 'PROVENANCE.json'
    assert not folder.is_symlink() and folder.is_dir()
    assert not provenance.is_symlink() and provenance.is_file()
    files = json.loads(provenance.read_text())['files']
    assert isinstance(files, dict) and files
    sources = [provenance]
    for name, record in sorted(files.items()):
        relative = PurePosixPath(name)
        assert relative.as_posix() == name and not relative.is_absolute()
        assert '..' not in relative.parts and '\\' not in name and name != provenance.name
        path = folder / name
        assert all(not folder.joinpath(*relative.parts[:i]).is_symlink()
                   for i in range(1, len(relative.parts) + 1)) and path.is_file(), name
        assert hashlib.sha256(path.read_bytes()).hexdigest() == record['sha256'], name
        sources.append(path)
    return sources


def check(root=ROOT):
    bundle = root / 'R/src/rust/workspace.tar.xz'
    with tarfile.open(bundle) as archive:
        members = archive.getmembers()
        assert sum(member.size for member in members) <= 16 * 1024**2
        names = [member.name for member in members]
        assert len(names) == len(set(names))
        for member in members:
            path = PurePosixPath(member.name)
            assert member.isfile() and not path.is_absolute() and '..' not in path.parts
        contents = {member.name: archive.extractfile(member).read() for member in members}
    
    assert contents['README.md'] == public_readme(root), 'Bundled R README differs from reviewed public prose'
    workspace = tomllib.loads(contents['Cargo.toml'].decode())['workspace']
    root_workspace = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']
    assert workspace['package'] == root_workspace['package']
    assert workspace['dependencies'] == root_workspace['dependencies']
    assert workspace['members'] == ['crates/eigentrail-core', 'crates/eigentrail-check', 'crates/eigentrail-r']
    assert tomllib.loads(contents['Cargo.toml'].decode())['patch']['crates-io'] == {'inari': {'path': 'backports/inari'}}
    fork = {p.relative_to(root).as_posix(): p for p in backport_sources(root / 'backports/inari')}
    assert {name for name in contents if name.startswith('backports/inari/')} == set(fork)
    for name, source in fork.items():
        assert contents[name] == source.read_bytes(), 'R backport source differs: ' + name
    expected = set()
    for member in workspace['members']:
        folder = root / member
        sources = [folder / 'Cargo.toml', *(folder / 'src').rglob('*.rs')]
        for name in ['LICENSE', 'build.rs']:
            if (folder / name).exists():
                sources.append(folder / name)
        for source in sources:
            name = source.relative_to(root).as_posix()
            expected_bytes = production_manifest(source.read_text()).encode() if source.name == 'Cargo.toml' else source.read_bytes()
            assert contents[name] == expected_bytes, 'R source differs: ' + name
            expected.add(name)
    assert {name for name in contents if name.startswith('crates/')} == expected
    
    root_lock = {(p['name'], p['version']): p for p in tomllib.loads((root / 'Cargo.lock').read_text())['package']}
    for package in tomllib.loads(contents['Cargo.lock'].decode())['package']:
        key = (package['name'], package['version'])
        assert key in root_lock
        if package['name'] == 'inari':
            assert 'source' not in package and 'checksum' not in package
        else:
            assert package.get('checksum') == root_lock[key].get('checksum')
    manifest = json.loads((root / 'R/inst/BUILD-SOURCES.json').read_text())
    for name, digest in manifest.items():
        assert hashlib.sha256((root / 'R' / name).read_bytes()).hexdigest() == digest, name
    print('Bundled R sources, dependency pins and package-file hashes match.')


if __name__ == '__main__':
    check()
