"""Check that the installable R package contains the current Rust sources."""
from pathlib import Path, PurePosixPath
import hashlib
import json
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
bundle = ROOT / 'R/src/rust/workspace.tar.xz'
with tarfile.open(bundle) as archive:
    members = archive.getmembers()
    assert sum(member.size for member in members) <= 16 * 1024**2
    names = [member.name for member in members]
    assert len(names) == len(set(names))
    for member in members:
        path = PurePosixPath(member.name)
        assert member.isfile() and not path.is_absolute() and '..' not in path.parts
    contents = {member.name: archive.extractfile(member).read() for member in members}

workspace = tomllib.loads(contents['Cargo.toml'].decode())['workspace']
root_workspace = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']
assert workspace['package'] == root_workspace['package']
assert workspace['dependencies'] == root_workspace['dependencies']
assert workspace['members'] == ['crates/eigentrail-core', 'crates/eigentrail-check', 'crates/eigentrail-r']
expected = set()
for member in workspace['members']:
    folder = ROOT / member
    sources = [folder / 'Cargo.toml', *(folder / 'src').rglob('*.rs')]
    for name in ['LICENSE', 'build.rs']:
        if (folder / name).exists():
            sources.append(folder / name)
    for source in sources:
        name = source.relative_to(ROOT).as_posix()
        assert contents[name] == source.read_bytes(), 'R source differs: ' + name
        expected.add(name)
assert {name for name in contents if name.startswith('crates/')} == expected

root_lock = {(p['name'], p['version']): p for p in tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']}
for package in tomllib.loads(contents['Cargo.lock'].decode())['package']:
    key = (package['name'], package['version'])
    assert key in root_lock and package.get('checksum') == root_lock[key].get('checksum')
manifest = json.loads((ROOT / 'R/inst/BUILD-SOURCES.json').read_text())
for name, digest in manifest.items():
    assert hashlib.sha256((ROOT / 'R' / name).read_bytes()).hexdigest() == digest, name
print('Bundled R sources, dependency pins and package-file hashes match.')
