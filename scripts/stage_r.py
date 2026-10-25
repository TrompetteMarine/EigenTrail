"""Prepare a source-only R package with vendored production Rust dependencies.

Maintainer commands (Cargo is deliberately run separately):
  python3 scripts/stage_r.py --prepare-vendor build/r-vendor-workspace
  cargo metadata --offline --format-version 1 --manifest-path build/r-vendor-workspace/Cargo.toml
  cargo vendor --locked --offline --versioned-dirs --manifest-path build/r-vendor-workspace/Cargo.toml build/r-vendor-workspace/vendor
  python3 scripts/stage_r.py --vendor-workspace build/r-vendor-workspace
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import tarfile
import tempfile
import tomllib

from dependency_inventory import notice_inputs

ROOT = Path(__file__).resolve().parents[1]
MEMBERS = tuple('crates/eigentrail-' + name for name in ('core', 'check', 'r'))
FORBIDDEN = {'eigentrail-python', 'pyo3', 'numpy', 'rug', 'gmp-mpfr-sys', 'rand_chacha'}
GENERATED = {'target', '__pycache__', '.git', '.cargo-home'}
BINARY_SUFFIXES = {'.o', '.a', '.so', '.dylib', '.dll', '.exe', '.pyc', '.pyo', '.rlib', '.rmeta'}


def source_files(folder):
    """Exclude generated files; never follow linked inputs."""
    for path in sorted(folder.rglob('*')):
        relative = path.relative_to(folder)
        if GENERATED.intersection(relative.parts):
            continue
        if path.is_symlink():
            raise ValueError('Linked source input: ' + str(path))
        if path.is_file() and path.suffix not in BINARY_SUFFIXES and path.name != 'Makevars':
            yield path


def production_manifest(text):
    """Remove validation dependencies in the copy, leaving source manifests intact."""
    lines, dropping = [], False
    for line in text.splitlines(keepends=True):
        if line.lstrip().startswith('['):
            table = line.strip().strip('[]').strip()
            dropping = table == 'dev-dependencies' or '.dev-dependencies' in table
        if not dropping:
            lines.append(line)
    result = ''.join(lines)
    assert 'dev-dependencies' not in tomllib.loads(result)
    return result


def workspace_manifest():
    original = (ROOT / 'Cargo.toml').read_text()
    result = re.sub(r'^members\s*=.*$', 'members = ' + json.dumps(MEMBERS), original, flags=re.M)
    result = re.sub(r'^default-members\s*=.*$', 'default-members = ["crates/eigentrail-r"]', result, flags=re.M)
    assert tuple(tomllib.loads(result)['workspace']['members']) == MEMBERS
    return result


def source_inputs():
    for name in ('README.md', 'LICENSE'):
        yield ROOT / name
    for member in MEMBERS:
        folder = ROOT / member
        yield folder / 'Cargo.toml'
        yield from source_files(folder / 'src')
        for name in ('build.rs', 'LICENSE'):
            if (folder / name).is_file():
                yield folder / name


def prepare_vendor(destination):
    destination = destination.resolve()
    destination.mkdir(parents=True, exist_ok=False)
    (destination / 'Cargo.toml').write_text(workspace_manifest())
    shutil.copyfile(ROOT / 'Cargo.lock', destination / 'Cargo.lock')
    for source in source_inputs():
        target = destination / source.relative_to(ROOT)
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.name == 'Cargo.toml':
            target.write_text(production_manifest(source.read_text()))
        else:
            shutil.copyfile(source, target)
    print(destination)
    print('Prepared core/check/R source only; run the Cargo commands in --help before staging.')


def checked_vendor(workspace):
    workspace = workspace.resolve()
    assert (workspace / 'Cargo.toml').read_text() == workspace_manifest(), 'Production workspace manifest changed'
    expected = set()
    for original in source_inputs():
        relative = original.relative_to(ROOT)
        expected.add(relative.as_posix())
        data = (production_manifest(original.read_text()).encode()
                if original.name == 'Cargo.toml' else original.read_bytes())
        assert (workspace / relative).read_bytes() == data, 'Prepared source differs: ' + str(relative)
    packages = tomllib.loads((workspace / 'Cargo.lock').read_text())['package']
    assert not FORBIDDEN.intersection(p['name'] for p in packages), 'Python/development dependencies remain in R lock'
    registry = [p for p in packages if p.get('source')]
    assert registry and all(p['source'].startswith('registry+') and p.get('checksum') for p in registry), 'Unsupported/unpinned dependency source'
    vendor = workspace / 'vendor'
    expected_directories = {p['name'] + '-' + p['version'] for p in registry}
    assert {p.name for p in vendor.iterdir()} == expected_directories, 'Vendor directory differs from production lock'
    for package in registry:
        folder = vendor / (package['name'] + '-' + package['version'])
        checksum = json.loads((folder / '.cargo-checksum.json').read_text())
        assert checksum['package'] == package['checksum'], 'Vendor crate checksum differs'
        files = list(source_files(folder))
        actual = {str(p.relative_to(folder)) for p in files if p.name != '.cargo-checksum.json'}
        assert actual == set(checksum['files']), 'Generated/binary/missing vendor file: ' + str(folder)
        for path in files:
            data = path.read_bytes()
            assert not data.startswith((b'\x7fELF', b'MZ', b'\xcf\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'!<arch>\n')), 'Executable/object in vendor source: ' + str(path)
            if path.name != '.cargo-checksum.json':
                assert hashlib.sha256(data).hexdigest() == checksum['files'][str(path.relative_to(folder))], 'Altered vendor source: ' + str(path)
    return workspace, expected


def copy_r_notices(inst, workspace):
    """Ship notices for the actual R lock, not the Python/development union."""
    locked = {(p['name'], p['version']) for p in
              tomllib.loads((workspace / 'Cargo.lock').read_text())['package']}
    inventory = json.loads((ROOT / 'docs/dependency-inventory.json').read_text())
    inventory['packages'] = [p for p in inventory['packages'] if (p['name'], p['version']) in locked]
    assert {(p['name'], p['version']) for p in inventory['packages']} == locked, 'R dependency license inventory is incomplete'
    for package in inventory['packages']:
        package.pop('features', None)
    inventory['production_graphs'] = {'eigentrail-r': inventory['production_graphs']['eigentrail-r']}
    inventory['workspace_features_scope'] = 'R-only production lock; enabled features are defined by the bundled Cargo manifests, not the former development feature union'
    inventory['native'] = []
    inventory['distribution_qualified'] = False
    (inst / 'dependency-inventory.json').write_text(json.dumps(inventory, indent=2) + '\n')
    origins = json.loads((ROOT / 'docs/license-sources.json').read_text())
    origins['recovered'] = [p for p in origins['recovered'] if (p['name'], p['version']) in locked]
    (inst / 'license-sources.json').write_text(json.dumps(origins, indent=2) + '\n')
    for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md'):
        shutil.copyfile(ROOT / name, inst / name)
    standard_library = Path('licenses/rust-standard-library-1.99.0/COPYRIGHT-library.html')
    (inst / standard_library).parent.mkdir(parents=True)
    shutil.copyfile(ROOT / standard_library, inst / standard_library)
    for package in inventory['packages']:
        if package['source']:
            relative = Path('licenses/dependencies') / (package['name'] + '-' + package['version'])
            assert (ROOT / relative).is_dir(), 'Missing R dependency notices: ' + str(relative)
            shutil.copytree(ROOT / relative, inst / relative)


def stage(workspace):
    workspace, expected = checked_vendor(workspace)
    r_sources = [p for p in source_files(ROOT / 'R')
                 if p != ROOT / 'R/tests/test_configure.py']
    sources = list(source_inputs()) + r_sources
    payload = set(sources) | set(notice_inputs().values()) | {Path(__file__), ROOT / 'scripts/dependency_inventory.py'}
    digest = hashlib.sha256()
    for path in sorted(payload):
        digest.update(str(path.relative_to(ROOT)).encode() + path.read_bytes())
    for path in [workspace / 'Cargo.toml', workspace / 'Cargo.lock', *source_files(workspace / 'vendor')]:
        digest.update(str(path.relative_to(workspace)).encode() + path.read_bytes())
    destination = ROOT / 'build' / ('eigentrail-' + digest.hexdigest()[:12])
    destination.mkdir(parents=True, exist_ok=False)
    for source in r_sources:
        target = destination / source.relative_to(ROOT / 'R')
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    rust = destination / 'src/rust'
    rust.mkdir()
    for relative in ['Cargo.toml', 'Cargo.lock', *sorted(expected)]:
        target = rust / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(workspace / relative, target)
    config = rust / '.cargo'
    config.mkdir()
    (config / 'config.toml').write_text(
        '[source.crates-io]\nreplace-with = "vendored-sources"\n'
        '[source.vendored-sources]\ndirectory = "vendor"\n'
        '[net]\noffline = true\n[build]\njobs = 2\n'
        '[target.x86_64-unknown-linux-gnu]\n'
        'rustflags = ["-C", "target-cpu=haswell"]\n')
    with tarfile.open(rust / 'vendor.tar.xz', 'w:xz') as archive:
        for path in source_files(workspace / 'vendor'):
            data = path.read_bytes()
            entry = tarfile.TarInfo('vendor/' + str(path.relative_to(workspace / 'vendor')))
            entry.size, entry.mode, entry.mtime = len(data), path.stat().st_mode & 0o777, 0
            archive.addfile(entry, io.BytesIO(data))
    (destination / 'configure').chmod(0o755)
    inst = destination / 'inst'
    inst.mkdir(exist_ok=True)
    copy_r_notices(inst, workspace)
    (inst / 'licenses/eigentrail').mkdir()
    shutil.copyfile(ROOT / 'LICENSE', inst / 'licenses/eigentrail/LICENSE')
    manifest = {str(p.relative_to(destination)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in source_files(destination)}
    (inst / 'BUILD-SOURCES.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(destination)


def self_test():
    sample = '[package]\nname="x"\n[dev-dependencies]\nrug="1"\n[dependencies]\nserde="1"\n'
    assert 'rug' not in production_manifest(sample)
    assert tomllib.loads(production_manifest(sample))['dependencies'] == {'serde': '1'}
    assert tuple(tomllib.loads(workspace_manifest())['workspace']['members']) == MEMBERS
    with tempfile.TemporaryDirectory() as directory:
        folder = Path(directory)
        (folder / 'source.rs').write_text('source')
        (folder / 'source.o').write_bytes(b'object')
        (folder / 'target').mkdir()
        (folder / 'target/generated.rs').write_text('generated')
        assert [p.name for p in source_files(folder)] == ['source.rs']
    print('R source preparation self-check passed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--prepare-vendor', type=Path)
    mode.add_argument('--vendor-workspace', type=Path)
    mode.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.prepare_vendor:
        prepare_vendor(args.prepare_vendor)
    elif args.vendor_workspace:
        stage(args.vendor_workspace)
    else:
        self_test()
