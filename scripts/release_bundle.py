"""Assemble and verify the immutable EigenTrail release bundle.

`create` runs in release-validation.yml after every platform job has passed. It
binds the exact staged sdist and the one allowlisted wheel to the source commit
and the workflow run, and writes release-receipt.json and SHA256SUMS beside them.

`verify` runs in publish-pypi.yml, in a job without PyPI credentials and from
the protected publishing branch, with the candidate commit checked out as data
under --root. It checks the validation run's metadata, the receipt, every hash
and the wheel tags. It then requires the private qualification receipt on the
GitHub release, as selected by qualified_artifacts.py, to approve exactly these
bytes, records the release's asset inventory for the post-approval re-check, and
copies only the allowlisted files into a dist directory.
"""
from email.parser import Parser
from pathlib import Path
import argparse
import gzip
import hashlib
import json
import re
import shutil
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
# v2: candidates are copied at build time. v1 receipts (commit 630be8f) copied
# them after the smoke tests, so they are refused.
SCHEMA = 'eigentrail.release-bundle.v2'
APPROVAL_SCHEMA = 'eigentrail.python-distribution.v1'  # What qualified_artifacts.py prints.
WORKFLOW = '.github/workflows/release-validation.yml'
RECEIPT = 'release-receipt.json'
SUMS = 'SHA256SUMS'
# Platform jobs in release-validation.yml and the wheel platform tag each builds.
PLATFORMS = {'macos-15-arm64': 'macosx_14_0_arm64', 'ubuntu-24.04-x86_64': 'linux_x86_64'}
WHEEL_PLATFORM = 'macos-15-arm64'       # PyPI rejects plain linux_x86_64 wheels.
SDIST_PLATFORM = 'ubuntu-24.04-x86_64'  # Linux installs build from the sdist.
PYTHON_TAG = 'cp314-cp314'
SOURCE_FILES = ('source-commit.txt', 'source-tree.txt')
HEX40 = re.compile(r'[0-9a-f]{40}')
HEX64 = re.compile(r'[0-9a-f]{64}')
CHUNK = 1 << 20


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(handle):
    value = hashlib.sha256()
    for block in iter(lambda: handle.read(CHUNK), b''):
        value.update(block)
    return value.hexdigest()


def sha256(path):
    with path.open('rb') as handle:
        return digest(handle)


def payload_sha256(path):
    """Hash the uncompressed tar stream, so runner zlib differences don't matter."""
    with gzip.open(path, 'rb') as handle:
        return digest(handle)


def sdist_name(version):
    return f'eigentrail-{version}.tar.gz'


def wheel_name(version, platform):
    return f'eigentrail-{version}-{PYTHON_TAG}-{PLATFORMS[platform]}.whl'


def allowlist(version):
    """Published file name -> (kind, platform job that built and tested it)."""
    return {sdist_name(version): ('sdist', SDIST_PLATFORM),
            wheel_name(version, WHEEL_PLATFORM): ('wheel', WHEEL_PLATFORM)}


def project_version(root):
    return tomllib.loads((root / 'python/pyproject.toml').read_text())['project']['version']


def flat_files(folder):
    entries = list(folder.iterdir())
    require(all(p.is_file() and not p.is_symlink() for p in entries), f'{folder.name} must hold only regular files')
    return {p.name for p in entries}


def check_wheel_tags(path, platform):
    """Reject universal2 and multi-platform wheels whatever the file is called.

    Read WHEEL as pip does: strict UTF-8, then the email parser, so header names
    are case-insensitive. Strip only the ASCII blanks a header value can carry:
    str.strip() would also drop U+00A0 and other Unicode spaces, accepting a tag
    that is not byte-for-byte the expected one.
    """
    with zipfile.ZipFile(path) as archive:
        records = [n for n in archive.namelist() if n.count('/') == 1 and n.endswith('.dist-info/WHEEL')]
        require(len(records) == 1, f'Expected one WHEEL record in {path.name}')
        require(archive.getinfo(records[0]).file_size <= 64 * 1024, 'Oversized WHEEL record')
        data = archive.read(records[0])
    try:
        text = data.decode('utf-8')
    except UnicodeDecodeError:
        raise ValueError(f'{path.name} WHEEL record is not UTF-8') from None
    tags = [str(tag).strip(' \t\r\n') for tag in Parser().parsestr(text).get_all('Tag', [])]
    require(tags == [f'{PYTHON_TAG}-{PLATFORMS[platform]}'], f'{path.name} declares wheel tags {tags}')


def load_json(path, what='release receipt'):
    def unique(pairs):
        keys = [key for key, _ in pairs]
        require(len(keys) == len(set(keys)), f'Duplicate key in {what}')
        return dict(pairs)
    value = json.loads(path.read_text(encoding='utf-8'), object_pairs_hook=unique)
    require(isinstance(value, dict), f'{what} must be a JSON object')
    return value


def sums_text(hashes):
    return ''.join(f'{hashes[name]}  {name}\n' for name in sorted(hashes))


def read_sums(path):
    sums = {}
    for line in path.read_text(encoding='ascii').splitlines():
        value, separator, name = line.partition('  ')
        require(separator and HEX64.fullmatch(value) and name and '/' not in name,
                f'Malformed {SUMS} line: {line!r}')
        require(name not in sums, f'Duplicate {SUMS} entry: {name}')
        sums[name] = value
    return sums


def check_bundle(bundle, *, repository, run_id, run_attempt, commit, tree, version):
    """Check a bundle against independently known facts; return name -> sha256."""
    allowed = allowlist(version)
    files = flat_files(bundle)
    expected_files = set(allowed) | {RECEIPT, SUMS}
    require(files == expected_files, f'Bundle files {sorted(files)} differ from the allowlist {sorted(expected_files)}')
    receipt = load_json(bundle / RECEIPT)
    facts = {'schema': SCHEMA, 'repository': repository, 'workflow': WORKFLOW, 'run_id': run_id,
             'run_attempt': run_attempt, 'commit': commit, 'tree': tree, 'version': version}
    require(set(receipt) == set(facts) | {'qualification', 'artifacts'},
            f'Unexpected receipt fields: {sorted(receipt)}')
    for key, value in facts.items():
        # Compare types too: JSON true would otherwise equal run attempt 1.
        require(type(receipt[key]) is type(value) and receipt[key] == value,
                f'Receipt {key} is {receipt[key]!r}, expected {value!r}')
    qualification, artifacts = receipt['qualification'], receipt['artifacts']
    require(isinstance(qualification, dict) and qualification.get('status') == 'passed'
            and isinstance(qualification.get('tested'), dict) and set(qualification['tested']) == set(PLATFORMS),
            'Receipt does not record a passed qualification on every platform')
    for platform in PLATFORMS:
        tested = qualification['tested'][platform]
        expected = {sdist_name(version), wheel_name(version, platform)}
        require(isinstance(tested, dict) and set(tested) == expected
                and all(isinstance(value, str) and HEX64.fullmatch(value) for value in tested.values()),
                f'Receipt qualification for {platform} does not list exactly {sorted(expected)}')
    require(isinstance(artifacts, dict) and set(artifacts) == set(allowed),
            f'Receipt artifacts {sorted(artifacts)} differ from the allowlist {sorted(allowed)}')
    sums = read_sums(bundle / SUMS)
    require(set(sums) == set(allowed), f'{SUMS} entries {sorted(sums)} differ from the allowlist {sorted(allowed)}')
    for name, (kind, platform) in allowed.items():
        entry, actual = artifacts[name], sha256(bundle / name)
        require(isinstance(entry, dict) and entry.get('kind') == kind and entry.get('platform') == platform,
                f'Receipt misdescribes {name}')
        require(entry.get('sha256') == sums[name] == actual == qualification['tested'][platform][name],
                f'SHA-256 mismatch for {name}')
        require(type(entry.get('size')) is int and entry['size'] == (bundle / name).stat().st_size,
                f'Size mismatch for {name}')
    require((bundle / SUMS).read_bytes() == sums_text(sums).encode('ascii'), f'{SUMS} is not in canonical form')
    sdist, wheel = sdist_name(version), wheel_name(version, WHEEL_PLATFORM)
    require(artifacts[sdist].get('payload_sha256') == payload_sha256(bundle / sdist), 'sdist payload mismatch')
    check_wheel_tags(bundle / wheel, WHEEL_PLATFORM)
    return {name: sums[name] for name in sorted(allowed)}


def read_candidate(folder, platform, *, commit, tree, version):
    expected = {sdist_name(version), wheel_name(version, platform), *SOURCE_FILES}
    files = flat_files(folder)
    require(files == expected, f'{platform} candidate files {sorted(files)} differ from {sorted(expected)}')
    require((folder / 'source-commit.txt').read_text().strip() == commit, f'{platform} built a different source commit')
    require((folder / 'source-tree.txt').read_text().strip() == tree, f'{platform} built a different source tree')
    check_wheel_tags(folder / wheel_name(version, platform), platform)
    return {name: sha256(folder / name) for name in sorted(expected - set(SOURCE_FILES))}


def create(candidates, out, *, repository, workflow_ref, ref, run_id, run_attempt, commit, tree, version):
    require(HEX40.fullmatch(commit) and HEX40.fullmatch(tree), 'Commit and tree must be full lowercase SHA-1')
    require(ref.startswith('refs/') and workflow_ref == f'{repository}/{WORKFLOW}@{ref}',
            f'Bundles come only from {WORKFLOW}')
    require(isinstance(run_id, int) and isinstance(run_attempt, int) and run_id > 0 and run_attempt > 0,
            'Run id and attempt must be positive integers')
    folders = {'candidate-' + platform: platform for platform in PLATFORMS}
    present = {p.name for p in candidates.iterdir()}
    require(present == set(folders), f'Candidate artifacts {sorted(present)} differ from {sorted(folders)}')
    tested = {platform: read_candidate(candidates / folder, platform, commit=commit, tree=tree, version=version)
              for folder, platform in folders.items()}
    sdist = sdist_name(version)
    payloads = {platform: payload_sha256(candidates / ('candidate-' + platform) / sdist) for platform in PLATFORMS}
    require(len(set(payloads.values())) == 1, f'Platforms tested different sdist contents: {payloads}')
    out.mkdir()
    artifacts = {}
    for name, (kind, platform) in allowlist(version).items():
        shutil.copyfile(candidates / ('candidate-' + platform) / name, out / name)
        artifacts[name] = {'kind': kind, 'platform': platform, 'sha256': sha256(out / name),
                           'size': (out / name).stat().st_size}
    artifacts[sdist]['payload_sha256'] = payloads[SDIST_PLATFORM]
    receipt = {'schema': SCHEMA, 'repository': repository, 'workflow': WORKFLOW, 'run_id': run_id,
               'run_attempt': run_attempt, 'commit': commit, 'tree': tree, 'version': version,
               'qualification': {'status': 'passed', 'tested': tested}, 'artifacts': artifacts}
    (out / RECEIPT).write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n', encoding='utf-8')
    (out / SUMS).write_text(sums_text({name: entry['sha256'] for name, entry in artifacts.items()}),
                            encoding='ascii')
    return check_bundle(out, repository=repository, run_id=run_id, run_attempt=run_attempt,
                        commit=commit, tree=tree, version=version)


def check_run(run, workflow, *, repository, run_id, commit):
    """Check GitHub's records of the validation workflow and run.

    workflow: GET /repos/{repo}/actions/workflows/release-validation.yml
    run:      GET /repos/{repo}/actions/runs/{run_id}
    """
    require(isinstance(workflow, dict) and workflow.get('path') == WORKFLOW and type(workflow.get('id')) is int,
            f'Workflow metadata is not for {WORKFLOW}')
    require(isinstance(run, dict) and type(run.get('id')) is int and run['id'] == run_id,
            'Run metadata is for a different run')
    require(type(run.get('workflow_id')) is int and run['workflow_id'] == workflow['id'],
            f'Run {run_id} is not {WORKFLOW}')
    require(run.get('event') == 'workflow_dispatch', f'Run {run_id} was not dispatched manually')
    require(run.get('status') == 'completed' and run.get('conclusion') == 'success', f'Run {run_id} did not succeed')
    require(run.get('head_sha') == commit, f'Run {run_id} validated {run.get("head_sha")}, not {commit}')
    for field in ('repository', 'head_repository'):
        require((run.get(field) or {}).get('full_name') == repository, f'Run {run_id} {field} is not {repository}')
    require(type(run.get('run_attempt')) is int, 'Run metadata lacks run_attempt')


def check_approval(approved, hashes):
    """Require the release's private qualification receipt to approve exactly the bundle's bytes.

    approved: the JSON qualified_artifacts.py prints once it has checked the GitHub
    release's receipt, checksums and assets for this commit and tree. A wheel the
    qualification rebuilt itself has a different hash and is refused.
    """
    require(isinstance(approved, dict) and set(approved) == {'schema', 'selected'}
            and approved['schema'] == APPROVAL_SCHEMA, 'Approval is not qualified_artifacts.py output')
    selected = approved['selected']
    require(isinstance(selected, dict) and selected == hashes,
            f'The release receipt approves {selected!r}, but the validation bundle holds {hashes!r}')


def verify(bundle, run, workflow, approved, *, repository, run_id, commit, tree, tag, version):
    require(HEX40.fullmatch(commit) and HEX40.fullmatch(tree), 'Commit and tree must be full lowercase SHA-1')
    require(tag == 'v' + version, f'Tag {tag} does not name version {version}')
    check_run(run, workflow, repository=repository, run_id=run_id, commit=commit)
    hashes = check_bundle(bundle, repository=repository, run_id=run_id, run_attempt=run['run_attempt'],
                          commit=commit, tree=tree, version=version)
    check_approval(approved, hashes)
    return hashes


ASSET_NAME = re.compile(r'[A-Za-z0-9_.-]+')  # qualified_artifacts.py's release filename rule


def release_inventory(release):
    """Canonical `sha256  name` lines for every GitHub release asset qualified_artifacts.py checked.

    The publish job downloads the whole release again after environment approval,
    rebuilds these lines in shell (hashing each file through a literal ./name path,
    sorted bytewise) and compares their SHA-256 with the one verify outputs. Any asset
    added, removed or changed while the approval was pending stops the upload.
    """
    for name in (RECEIPT, SUMS):
        path = release / name
        require(path.is_file() and not path.is_symlink(), f'The GitHub release has no regular {name}')
    names = sorted(flat_files(release))
    require(all(ASSET_NAME.fullmatch(name) for name in names), 'Unsafe GitHub release asset name')
    return ''.join(f'{sha256(release / name)}  {name}\n' for name in names)


def inventory_sha256(text):
    return hashlib.sha256(text.encode('ascii')).hexdigest()


def stage(bundle, dist, hashes):
    """Copy only the verified files into a new dist directory and re-hash the copies."""
    dist.mkdir()
    for name, value in hashes.items():
        shutil.copyfile(bundle / name, dist / name)
        require(sha256(dist / name) == value, f'{name} changed while staging')
    require(flat_files(dist) == set(hashes), 'dist holds unexpected files')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('create', 'verify'):
        command = commands.add_parser(name)
        command.add_argument('--root', type=Path, default=ROOT)
        command.add_argument('--repository', required=True)
        command.add_argument('--run-id', type=int, required=True)
        command.add_argument('--commit', required=True)
        command.add_argument('--tree', required=True)
    create_command, verify_command = commands.choices['create'], commands.choices['verify']
    create_command.add_argument('--candidates', type=Path, required=True)
    create_command.add_argument('--out', type=Path, required=True)
    create_command.add_argument('--workflow-ref', required=True, help='GITHUB_WORKFLOW_REF')
    create_command.add_argument('--ref', required=True, help='GITHUB_REF')
    create_command.add_argument('--run-attempt', type=int, required=True)
    verify_command.add_argument('--bundle', type=Path, required=True)
    verify_command.add_argument('--run', type=Path, required=True, help='JSON from the GitHub runs API')
    verify_command.add_argument('--workflow', type=Path, required=True, help='JSON from the GitHub workflows API')
    verify_command.add_argument('--approved', type=Path, required=True,
                                help='JSON that qualified_artifacts.py printed for the GitHub release')
    verify_command.add_argument('--release', type=Path, required=True,
                                help='The GitHub release assets qualified_artifacts.py checked')
    verify_command.add_argument('--tag', required=True)
    verify_command.add_argument('--dist', type=Path, required=True)
    verify_command.add_argument('--github-output', type=Path)
    args = parser.parse_args()
    version = project_version(args.root)
    common = dict(repository=args.repository, run_id=args.run_id, commit=args.commit, tree=args.tree, version=version)
    try:
        if args.command == 'create':
            hashes = create(args.candidates, args.out, workflow_ref=args.workflow_ref, ref=args.ref,
                            run_attempt=args.run_attempt, **common)
        else:
            run = load_json(args.run, 'run metadata')
            workflow = load_json(args.workflow, 'workflow metadata')
            approved = load_json(args.approved, 'approval')
            hashes = verify(args.bundle, run, workflow, approved, tag=args.tag, **common)
            inventory = release_inventory(args.release)
            stage(args.bundle, args.dist, hashes)
    except ValueError as error:
        raise SystemExit(f'release bundle {args.command} failed: {error}')
    summary = {'status': 'passed', 'command': args.command, 'version': version, 'sha256': hashes}
    if args.command == 'verify':
        summary['release_inventory'] = inventory.splitlines()
        summary['validation_run_attempt'] = run['run_attempt']
        if args.github_output:
            sdist, wheel = sdist_name(version), wheel_name(version, WHEEL_PLATFORM)
            with args.github_output.open('a', encoding='ascii') as output:
                output.write(f'sdist={sdist}\nsdist_sha256={hashes[sdist]}\nwheel={wheel}\nwheel_sha256={hashes[wheel]}\n'
                             f'release_inventory_sha256={inventory_sha256(inventory)}\n'
                             f'validation_run_attempt={run["run_attempt"]}\n')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
