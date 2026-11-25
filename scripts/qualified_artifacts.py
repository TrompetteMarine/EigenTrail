"""Select exact qualified CPython 3.14 distributions; legacy receipts fail closed.

The caller verifies the Git tag and non-draft GitHub release. This selector checks
the supplied commit/tree, complete release directory and checksums, explicit
python_distribution allowlist, native target metadata and wheel-internal tags.
It publishes nothing and copies files only to a fresh destination.
"""
import argparse
from hashlib import sha256
import json
from pathlib import Path
import re
import shutil
import tomllib
from zipfile import ZipFile

SCHEMA = 'eigentrail.python-distribution.v1'
HEX = re.compile(r'[0-9a-f]{64}')
SHA = re.compile(r'[0-9a-f]{40}')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    require(path.is_file() and not path.is_symlink(), 'Regular release file required: ' + path.name)
    result = sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()


def load_json(path):
    def pairs(values):
        result = {}
        for key, value in values:
            require(key not in result, 'Duplicate receipt field: ' + key)
            result[key] = value
        return result
    return json.loads(path.read_text(), object_pairs_hook=pairs)


def target_for(tag):
    if re.fullmatch(r'cp314-cp314-macosx_[0-9]+_[0-9]+_arm64', tag):
        return 'macos', 'arm64'
    if tag == 'cp314-cp314-win_amd64':
        return 'windows', 'x86_64'
    if re.fullmatch(r'cp314-cp314-manylinux_(?:2_[0-9]+|2014)_x86_64', tag):
        return 'linux', 'x86_64'
    raise ValueError('Unsupported, compressed or wrong-ABI wheel tag: ' + tag)


def qualified(entry):
    require(entry.get('qualification') == 'passed' and entry.get('publication') == 'approved'
            and entry.get('candidate_only') is False, 'Unqualified or candidate-only Python artifact')
    require(isinstance(entry.get('qualification_receipt_sha256'), str)
            and HEX.fullmatch(entry['qualification_receipt_sha256']), 'Missing qualification receipt digest')


def headers(text):
    result = {}
    for line in text.splitlines():
        if not line:
            break
        if line[:1].isspace():
            continue
        key, separator, value = line.partition(':')
        require(separator, 'Malformed wheel metadata header')
        result.setdefault(key.lower(), []).append(value.strip())
    return result


def inspect_wheel(path, version, tag):
    with ZipFile(path) as archive:
        names = archive.namelist()
        require(len(names) == len(set(names)), 'Repeated wheel archive member')
        folder = 'eigentrail-' + version + '.dist-info/'
        require([name for name in names if name.endswith('.dist-info/WHEEL')] == [folder + 'WHEEL'],
                'Wrong wheel distribution/version metadata directory')
        contents = {}
        for name in ('WHEEL', 'METADATA'):
            info = archive.getinfo(folder + name)
            require(info.file_size <= 1024 * 1024, 'Oversized wheel metadata')
            contents[name] = headers(archive.read(info).decode('utf-8'))
        require(contents['WHEEL'].get('tag') == [tag]
                and contents['WHEEL'].get('root-is-purelib') == ['false'], 'Wheel internal ABI/tag differs from qualified filename')
        require(contents['METADATA'].get('name') == ['eigentrail']
                and contents['METADATA'].get('version') == [version], 'Wheel internal package/version differs')


def select(directory, project, commit, tree):
    require(SHA.fullmatch(commit) and SHA.fullmatch(tree), 'Full commit and tree required')
    receipt = load_json(directory / 'release-receipt.json')
    require(receipt.get('commit') == commit and receipt.get('tree') == tree, 'Release source binding differs')
    require(receipt.get('pypi_ready') is True and receipt.get('qualification', {}).get('status') == 'passed',
            'Release is not qualified for production PyPI')
    expected = receipt['artifacts']; require(isinstance(expected, dict) and expected, 'Missing artifact inventory')
    require(all(path.is_file() and not path.is_symlink() for path in directory.iterdir()), 'Only regular release files are permitted')
    manifest = {}
    for line in (directory / 'SHA256SUMS').read_text().splitlines():
        checksum, separator, name = line.partition('  ')
        require(separator and HEX.fullmatch(checksum) and re.fullmatch(r'[A-Za-z0-9_.-]+', name)
                and name not in manifest, 'Malformed or duplicate checksum entry')
        manifest[name] = checksum
    require(manifest == {name: entry['sha256'] for name, entry in expected.items()}, 'Checksum manifest differs from receipt')
    require({path.name for path in directory.iterdir()} == set(expected) | {'SHA256SUMS', 'release-receipt.json'},
            'Missing or extra release files')
    for name, entry in expected.items():
        require(re.fullmatch(r'[A-Za-z0-9_.-]+', name) and name not in ('SHA256SUMS', 'release-receipt.json'), 'Unsafe artifact filename')
        require(entry.get('qualification') == 'passed' and HEX.fullmatch(entry['sha256']), 'Unqualified release artifact')
        require(digest(directory / name) == entry['sha256'], 'Artifact digest differs: ' + name)
    metadata = tomllib.loads(project.read_text())['project']
    version = metadata['version']
    require(metadata['name'] == 'eigentrail' and re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version), 'Unsupported project/version')
    policy = receipt.get('python_distribution')
    require(isinstance(policy, dict) and set(policy) == {'schema', 'wheel_tags', 'sdist'}
            and policy['schema'] == SCHEMA, 'Explicit python_distribution v1 required; legacy receipts are unsupported')
    tags = policy['wheel_tags']
    require(isinstance(tags, list) and tags and all(isinstance(tag, str) for tag in tags)
            and len(tags) == len(set(tags)), 'Missing or duplicate qualified wheel tags')
    source = 'eigentrail-' + version + '.tar.gz'
    require(policy['sdist'] == source and source in expected, 'Exactly one version-matched Python sdist required')
    sources = [name for name in expected if re.fullmatch(r'eigentrail-[0-9].*\.(?:tar\.gz|zip)', name)]
    require(sources == [source], 'Extra Python source distribution')
    qualified(expected[source])
    wheels = {}
    for name, entry in expected.items():
        if not name.endswith('.whl'):
            continue
        match = re.fullmatch(r'eigentrail-' + re.escape(version) + r'(?:-[0-9][A-Za-z0-9_]*)?-(cp314-cp314-[A-Za-z0-9_]+)\.whl', name)
        require(match is not None, 'Wrong package/version/ABI wheel: ' + name)
        tag = match[1]; require(tag in tags and tag not in wheels, 'Extra or duplicate qualified wheel tag')
        os_name, architecture = target_for(tag)
        qualified(entry)
        target = entry.get('target', {})
        require(set(target) == {'os', 'architecture', 'abi', 'minimum_cpu', 'numerical_backend'}
                and target['os'] == os_name and target['architecture'] == architecture and target['abi'] == 'cp314',
                'Wheel target tuple differs from tag')
        require(all(isinstance(value, str) and value.strip() for value in target.values()), 'Incomplete target qualification tuple')
        require(not any(word in target['numerical_backend'].lower() for word in ('unknown', 'pending', 'unqualified', 'candidate')),
                'Unqualified numerical backend')
        require(os_name != 'linux' or target['minimum_cpu'] == 'x86-64-v1', 'Generic PyPI Linux wheels require a qualified baseline CPU backend')
        require(os_name != 'macos' or target['minimum_cpu'] == 'arm64', 'Wrong macOS minimum CPU tuple')
        require(os_name != 'windows' or target['minimum_cpu'] in ('x86-64-v1', 'haswell+os-avx'), 'Wrong Windows minimum CPU tuple')
        require(entry.get('wheel_tag') == tag, 'Wheel tag is not bound by artifact entry')
        inspect_wheel(directory / name, version, tag)
        wheels[tag] = name
    require(set(wheels) == set(tags), 'Missing qualified wheel')
    return [directory / wheels[tag] for tag in tags] + [directory / source]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--project', type=Path, required=True)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--tree', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    source, destination = args.directory.resolve(), args.out.resolve()
    selected = select(source, args.project, args.commit, args.tree)
    require(not destination.exists() and not destination.is_relative_to(source), 'Fresh separate destination required')
    destination.mkdir(parents=True)
    for path in selected:
        target = destination / path.name
        with target.open('xb') as output, path.open('rb') as original:
            shutil.copyfileobj(original, output)
        require(digest(target) == digest(path), 'Copied distribution differs')
    print(json.dumps({'schema': SCHEMA, 'selected': {path.name: digest(path) for path in selected}}, sort_keys=True))


if __name__ == '__main__':
    main()
