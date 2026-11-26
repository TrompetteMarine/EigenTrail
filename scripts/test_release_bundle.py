"""Check release bundle provenance, exact artifact allowlisting and tamper refusal."""
from pathlib import Path
import gzip
import hashlib
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_bundle as rb  # noqa: E402

QUALIFIED_ARTIFACTS = Path(__file__).resolve().with_name('qualified_artifacts.py')
PUBLISH_WORKFLOW = Path(__file__).resolve().parents[1] / '.github/workflows/publish-pypi.yml'
VERSION = '0.1.0'
REPO = 'TrompetteMarine/EigenTrail'
COMMIT, TREE, OTHER = 'a' * 40, 'b' * 40, 'c' * 40
RUN_ID, ATTEMPT, WORKFLOW_ID = 4242, 1, 77
REF = 'refs/heads/main'
WORKFLOW_REF = f'{REPO}/{rb.WORKFLOW}@{REF}'
SHADOW = rb.WORKFLOW + '@alternate.yml'  # A different file whose name starts with the real one.
SDIST = rb.sdist_name(VERSION)
WHEEL = rb.wheel_name(VERSION, rb.WHEEL_PLATFORM)
LINUX_WHEEL = rb.wheel_name(VERSION, rb.SDIST_PLATFORM)
WINDOWS_PLATFORM = 'windows-2022-x86_64'
WINDOWS_WHEEL = rb.wheel_name(VERSION, WINDOWS_PLATFORM)
MAC_TAG = 'cp314-cp314-macosx_14_0_arm64'
LINUX_TAG = 'cp314-cp314-manylinux_2_28_x86_64'
WINDOWS_TAG = 'cp314-cp314-win_amd64'
UNIVERSAL2 = f'eigentrail-{VERSION}-cp314-cp314-macosx_14_0_universal2.whl'
ARM64_TAG = 'cp314-cp314-macosx_14_0_arm64'
R_SOURCE = f'eigentrail_{VERSION}.tar.gz'


def write_wheel(path, tags, extra=b''):
    with zipfile.ZipFile(path, 'w') as archive:
        archive.writestr(f'eigentrail-{VERSION}.dist-info/WHEEL', ('Wheel-Version: 1.0\nRoot-Is-Purelib: false\n'
                         + ''.join(f'Tag: {tag}\n' for tag in tags)).encode() + extra)
        archive.writestr(f'eigentrail-{VERSION}.dist-info/METADATA',
                         f'Metadata-Version: 2.4\nName: eigentrail\nVersion: {VERSION}\n')
        archive.writestr('eigentrail/__init__.py', '')


# Wheels that must be refused even when named ...-macosx_14_0_arm64.whl: (tags, extra WHEEL bytes, error).
BAD_WHEELS = [(['cp314-cp314-macosx_14_0_universal2'], b'', 'declares wheel tags'),
              (['cp314-cp314-macosx_14_0_x86_64', 'cp314-cp314-macosx_14_0_arm64'], b'', 'declares wheel tags'),
              (['cp314-cp314-macosx_14_0_arm64'], b'tag: cp314-cp314-macosx_14_0_universal2\n', 'declares wheel tags'),
              (['cp314-cp314-macosx_14_0_arm64'], b'Generator: \xff\n', 'not UTF-8'),  # pip refuses these too.
              # Unicode padding that str.strip() removes: the tag must be the expected one byte for byte.
              (['cp314-cp314-macosx_14_0_arm64 '], b'', 'declares wheel tags'),
              ([' cp314-cp314-macosx_14_0_arm64'], b'', 'declares wheel tags'),
              (['cp314-cp314-macosx_14_0_arm64　'], b'', 'declares wheel tags')]


def write_sdist(path, text='source', level=9):
    payload = io.BytesIO()
    with tarfile.open(fileobj=payload, mode='w') as archive:
        data = text.encode()
        info = tarfile.TarInfo(f'eigentrail-{VERSION}/PKG-INFO')
        info.size = len(data)
        archive.addfile(info, io.BytesIO(data))
    path.write_bytes(gzip.compress(payload.getvalue(), compresslevel=level, mtime=0))


def run_record(**changes):
    run = {'id': RUN_ID, 'run_attempt': ATTEMPT, 'workflow_id': WORKFLOW_ID, 'path': rb.WORKFLOW,
           'event': 'workflow_dispatch', 'status': 'completed', 'conclusion': 'success', 'head_sha': COMMIT,
           'repository': {'full_name': REPO}, 'head_repository': {'full_name': REPO}}
    return {**run, **changes}


def workflow_record(**changes):
    return {'id': WORKFLOW_ID, 'path': rb.WORKFLOW, 'state': 'active', **changes}


def approval(hashes):
    """What qualified_artifacts.py prints when the release receipt approves exactly these files."""
    return {'schema': rb.APPROVAL_SCHEMA, 'selected': dict(hashes)}


class Fixture(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.tmp = Path(folder.name)
        self.candidates = self.tmp / 'candidates'
        self.bundle = self.tmp / 'bundle'
        self.hashes = None
        for platform, tag in rb.PLATFORMS.items():
            candidate = self.candidate(platform)
            candidate.mkdir(parents=True)
            write_wheel(candidate / rb.wheel_name(VERSION, platform), [f'{rb.PYTHON_TAG}-{tag}'])
            if platform in rb.SDIST_PLATFORMS:
                # Same tar payload, different gzip bytes, as runners with different zlib builds give.
                write_sdist(candidate / SDIST, level=9 if platform == rb.SDIST_PLATFORM else 1)
            (candidate / 'source-commit.txt').write_text(COMMIT + '\n')
            (candidate / 'source-tree.txt').write_text(TREE + '\n')

    def candidate(self, platform):
        return self.candidates / ('candidate-' + platform)

    def create(self, **changes):
        args = dict(repository=REPO, workflow_ref=WORKFLOW_REF, ref=REF, run_id=RUN_ID, run_attempt=ATTEMPT,
                    commit=COMMIT, tree=TREE, version=VERSION)
        self.hashes = rb.create(self.candidates, self.bundle, **{**args, **changes})
        return self.hashes

    def verify(self, run=None, workflow=None, approved=None, **changes):
        """Verify as publish-pypi.yml does; by default the release approves the bundle as created."""
        args = dict(repository=REPO, run_id=RUN_ID, commit=COMMIT, tree=TREE, tag='v' + VERSION, version=VERSION)
        return rb.verify(self.bundle, run or run_record(), workflow or workflow_record(),
                         approval(self.hashes) if approved is None else approved, **{**args, **changes})

    def rejects(self, pattern, action, *args, **kwargs):
        with self.assertRaisesRegex(ValueError, pattern):
            action(*args, **kwargs)


class CreateTests(Fixture):
    def test_bundle_holds_only_allowlisted_bytes_bound_to_commit_and_run(self):
        linux, mac = (self.candidate(p) / SDIST for p in (rb.SDIST_PLATFORM, rb.WHEEL_PLATFORM))
        self.assertNotEqual(linux.read_bytes(), mac.read_bytes())
        hashes = self.create()
        self.assertEqual({p.name for p in self.bundle.iterdir()},
                         {SDIST, WHEEL, LINUX_WHEEL, WINDOWS_WHEEL, rb.RECEIPT, rb.SUMS})
        self.assertEqual((self.bundle / SDIST).read_bytes(), linux.read_bytes())
        receipt = json.loads((self.bundle / rb.RECEIPT).read_text())
        self.assertEqual((receipt['commit'], receipt['tree'], receipt['run_id'], receipt['run_attempt']),
                         (COMMIT, TREE, RUN_ID, ATTEMPT))
        self.assertIn(LINUX_WHEEL, receipt['ci_validation']['tested'][rb.SDIST_PLATFORM])
        self.assertEqual(self.verify(), hashes)

    def test_rejects_wrong_source_commit_or_tree(self):
        (self.candidate(rb.WHEEL_PLATFORM) / 'source-commit.txt').write_text(OTHER + '\n')
        self.rejects('different source commit', self.create)
        (self.candidate(rb.WHEEL_PLATFORM) / 'source-commit.txt').write_text(COMMIT + '\n')
        (self.candidate(rb.SDIST_PLATFORM) / 'source-tree.txt').write_text(OTHER + '\n')
        self.rejects('different source tree', self.create)

    def test_rejects_missing_extra_or_duplicate_wheels(self):
        mac = self.candidate(rb.WHEEL_PLATFORM)
        (mac / WHEEL).rename(self.tmp / WHEEL)
        self.rejects('candidate files', self.create)
        (self.tmp / WHEEL).rename(mac / WHEEL)
        for extra in [f'eigentrail-{VERSION}-cp314-cp314-macosx_14_0_x86_64.whl',
                      f'eigentrail-{VERSION}-1-{ARM64_TAG}.whl']:  # Build-tag duplicate of the same wheel.
            with self.subTest(extra=extra):
                shutil.copyfile(mac / WHEEL, mac / extra)
                self.rejects('candidate files', self.create)
                (mac / extra).unlink()

    def test_rejects_universal2_wheels(self):
        mac = self.candidate(rb.WHEEL_PLATFORM)
        (mac / WHEEL).rename(mac / UNIVERSAL2)
        self.rejects('candidate files', self.create)
        (mac / UNIVERSAL2).rename(mac / WHEEL)
        for tags, extra, error in BAD_WHEELS:
            with self.subTest(tags=tags, extra=extra):
                shutil.rmtree(self.bundle, ignore_errors=True)  # A wrongly accepted case must not mask the next.
                write_wheel(mac / WHEEL, tags, extra)
                self.rejects(error, self.create)

    def test_every_platform_wheel_must_have_its_exact_internal_tag(self):
        for platform, tag in rb.PLATFORMS.items():
            with self.subTest(platform=platform):
                path = self.candidate(platform) / rb.wheel_name(VERSION, platform)
                write_wheel(path, [f'{rb.PYTHON_TAG}-{tag}-extra'])
                self.rejects('declares wheel tags', self.create)

    def test_rejects_missing_or_extra_platforms(self):
        shutil.rmtree(self.candidate(rb.SDIST_PLATFORM))
        self.rejects('Candidate artifacts', self.create)
        self.candidate('windows-x86_64').mkdir()
        self.rejects('Candidate artifacts', self.create)

    def test_rejects_sdists_with_different_contents(self):
        write_sdist(self.candidate(rb.WHEEL_PLATFORM) / SDIST, text='changed')
        self.rejects('different sdist contents', self.create)

    def test_rejects_other_workflows(self):
        for workflow_ref in [f'{REPO}/.github/workflows/other.yml@{REF}', f'fork/EigenTrail/{rb.WORKFLOW}@{REF}',
                             f'{REPO}/{SHADOW}@{REF}', f'{REPO}/{rb.WORKFLOW}@refs/heads/other']:
            with self.subTest(workflow_ref=workflow_ref):
                self.rejects('Bundles come only', self.create, workflow_ref=workflow_ref)


class VerifyTests(Fixture):
    def setUp(self):
        super().setUp()
        self.create()

    def fresh(self):
        shutil.rmtree(self.bundle)
        self.create()

    def receipt(self):
        return json.loads((self.bundle / rb.RECEIPT).read_text())

    def write(self, receipt):
        (self.bundle / rb.RECEIPT).write_text(json.dumps(receipt))
        (self.bundle / rb.SUMS).write_text(''.join(f'{entry["sha256"]}  {name}\n'
                                                   for name, entry in sorted(receipt['artifacts'].items())))

    def rehash(self, name, rename=None):
        """Make receipt and sums consistent with the file's current bytes, as a careful forger would."""
        receipt = self.receipt()
        entry = receipt['artifacts'].pop(name)
        tested = receipt['ci_validation']['tested'][entry['platform']]
        del tested[name]
        if rename:
            (self.bundle / name).rename(self.bundle / rename)
            name = rename
        entry['sha256'] = tested[name] = rb.sha256(self.bundle / name)
        entry['size'] = (self.bundle / name).stat().st_size
        receipt['artifacts'][name] = entry
        self.write(receipt)

    def test_stages_only_verified_bytes(self):
        hashes = self.verify()
        rb.stage(self.bundle, self.tmp / 'dist', hashes)
        self.assertEqual({p.name: rb.sha256(p) for p in (self.tmp / 'dist').iterdir()}, hashes)

    def test_rejects_tampered_receipts(self):
        def edit(path, value):
            def change(receipt):
                *parents, key = path
                target = receipt
                for parent in parents:
                    target = target[parent]
                target[key] = value
            return change

        def drop(platform, name):
            return lambda receipt: receipt['ci_validation']['tested'][platform].pop(name)
        cases = [('Receipt commit', edit(['commit'], OTHER)),
                 ('Receipt run_id', edit(['run_id'], RUN_ID + 1)),
                 ('Receipt run_attempt', edit(['run_attempt'], ATTEMPT + 1)),
                 ('Receipt run_attempt', edit(['run_attempt'], True)),  # JSON true == 1 in Python.
                 ('CI checks for', drop(rb.SDIST_PLATFORM, LINUX_WHEEL)),
                 ('CI checks for', drop(rb.WHEEL_PLATFORM, SDIST)),
                 ('CI checks for', edit(['ci_validation', 'tested', rb.WHEEL_PLATFORM, SDIST], 'unchecked')),
                 ('Receipt workflow', edit(['workflow'], '.github/workflows/other.yml')),
                 ('Receipt schema', edit(['schema'], 'eigentrail.release-bundle.v2')),  # Two-platform bundles.
                 ('passed CI checks', edit(['ci_validation', 'status'], 'failed')),
                 ('passed CI checks', edit(['ci_validation', 'tested'], {rb.WHEEL_PLATFORM: {}})),
                 ('Unexpected receipt fields', edit(['note'], 'trust me')),
                 ('Receipt artifacts', edit(['artifacts', 'unlisted.whl'], {'kind': 'wheel'})),
                 ('misdescribes', edit(['artifacts', WHEEL, 'platform'], rb.SDIST_PLATFORM)),
                 ('SHA-256 mismatch', edit(['artifacts', WHEEL, 'sha256'], '0' * 64)),
                 ('SHA-256 mismatch', edit(['ci_validation', 'tested', rb.WHEEL_PLATFORM, WHEEL], '0' * 64)),
                 ('Size mismatch', edit(['artifacts', SDIST, 'size'], 1)),
                 ('payload mismatch', edit(['artifacts', SDIST, 'payload_sha256'], '0' * 64))]
        for pattern, change in cases:
            with self.subTest(pattern=pattern):
                self.fresh()
                receipt = self.receipt()
                change(receipt)
                (self.bundle / rb.RECEIPT).write_text(json.dumps(receipt))
                self.rejects(pattern, self.verify)
        self.fresh()
        text = (self.bundle / rb.RECEIPT).read_text()
        (self.bundle / rb.RECEIPT).write_text(text.replace('{', '{"commit": "%s", ' % COMMIT, 1))
        self.rejects('Duplicate key', self.verify)

    def test_rejects_wrong_hashes(self):
        with (self.bundle / WHEEL).open('ab') as wheel:
            wheel.write(b'\0')
        self.rejects('SHA-256 mismatch', self.verify)
        self.fresh()
        sums = (self.bundle / rb.SUMS).read_text()
        (self.bundle / rb.SUMS).write_text(sums.replace(rb.sha256(self.bundle / SDIST), '0' * 64))
        self.rejects('SHA-256 mismatch', self.verify)
        self.fresh()
        for separator in ('\v', '\r\n'):  # Python text reading accepts both; the canonical form is LF only.
            with self.subTest(separator=separator):
                self.fresh()
                (self.bundle / rb.SUMS).write_bytes(sums.replace('\n', separator).encode('ascii'))
                self.rejects('not in canonical form', self.verify)
        self.fresh()
        write_sdist(self.bundle / SDIST, text='changed')
        self.rehash(SDIST)
        self.rejects('payload mismatch', self.verify)

    def test_rejects_wrong_source_commit_or_tree(self):
        self.rejects('Receipt commit', self.verify, run_record(head_sha=OTHER), commit=OTHER)
        self.rejects('validated', self.verify, run_record(head_sha=OTHER))
        self.rejects('Receipt tree', self.verify, tree=OTHER)
        self.rejects('full lowercase SHA-1', self.verify, commit=COMMIT[:12])

    def test_rejects_missing_extra_or_duplicate_wheels(self):
        (self.bundle / WHEEL).unlink()
        self.rejects('Bundle files', self.verify)
        self.fresh()
        shutil.copyfile(self.candidate(rb.SDIST_PLATFORM) / LINUX_WHEEL,
                        self.bundle / f'eigentrail-{VERSION}-cp314-cp314-manylinux_2_28_aarch64.whl')
        self.rejects('Bundle files', self.verify)
        self.fresh()
        sums = (self.bundle / rb.SUMS).read_text()
        (self.bundle / rb.SUMS).write_text(sums + sums.splitlines(keepends=True)[-1])
        self.rejects('Duplicate SHA256SUMS entry', self.verify)
        self.fresh()
        (self.bundle / rb.SUMS).write_text(sums.replace(WHEEL, 'python/' + WHEEL))
        self.rejects('Malformed SHA256SUMS', self.verify)

    def test_rejects_universal2_wheels(self):
        self.rehash(WHEEL, rename=UNIVERSAL2)
        self.rejects('Bundle files', self.verify)
        for tags, extra, error in BAD_WHEELS:
            with self.subTest(tags=tags, extra=extra):
                self.fresh()
                write_wheel(self.bundle / WHEEL, tags, extra)
                self.rehash(WHEEL)
                self.rejects(error, self.verify)

    def test_rejects_unsuccessful_or_foreign_runs(self):
        cases = [('did not succeed', run_record(conclusion='failure'), None),
                 ('did not succeed', run_record(status='in_progress', conclusion=None), None),
                 ('is not', run_record(workflow_id=WORKFLOW_ID + 1), None),
                 ('is not', run_record(workflow_id=float(WORKFLOW_ID)), None),
                 ('Workflow metadata', None, workflow_record(path=SHADOW)),
                 ('dispatched manually', run_record(event='push'), None),
                 ('head_repository', run_record(head_repository={'full_name': 'fork/EigenTrail'}), None),
                 ('different run', run_record(id=RUN_ID + 1), None),
                 ('lacks run_attempt', run_record(run_attempt=True), None),
                 ('Receipt run_attempt', run_record(run_attempt=ATTEMPT + 1), None)]
        for pattern, run, workflow in cases:
            with self.subTest(pattern=pattern):
                self.rejects(pattern, self.verify, run, workflow)

    def test_rejects_tag_for_another_version(self):
        self.rejects('does not name version', self.verify, tag='v0.1.1')

    def test_requires_the_release_receipt_to_approve_exactly_the_bundle_bytes(self):
        hashes = self.hashes
        cases = [('approves', approval({**hashes, WHEEL: '1' * 64})),  # The qualification rebuilt the wheel.
                 ('approves', approval({SDIST: hashes[SDIST]})),  # The bundle's wheel is not approved.
                 ('approves', approval({**hashes, f'eigentrail-{VERSION}-cp314-cp314-win_amd64.whl': '2' * 64})),
                 ('approves', {'schema': rb.APPROVAL_SCHEMA, 'selected': sorted(hashes.items())}),
                 ('not qualified_artifacts.py output', {**approval(hashes), 'schema': 'eigentrail.python-distribution.v0'}),
                 ('not qualified_artifacts.py output', {**approval(hashes), 'note': 'trust me'}),
                 ('not qualified_artifacts.py output', {'selected': hashes})]
        for pattern, approved in cases:
            with self.subTest(approved=approved):
                self.rejects(pattern, self.verify, approved=approved)
        self.assertEqual(self.verify(approved=approval(hashes)), hashes)


class CommandLineFixture(Fixture):
    def setUp(self):
        super().setUp()
        self.root = self.tmp / 'root'  # Stands in for the candidate checkout publish-pypi.yml reads as data.
        (self.root / 'python').mkdir(parents=True)
        self.set_version(VERSION)
        self.script = str(Path(rb.__file__))
        self.common = ['--root', str(self.root), '--repository', REPO, '--run-id', str(RUN_ID), '--commit', COMMIT,
                       '--tree', TREE]
        (self.tmp / 'run.json').write_text(json.dumps(run_record()))
        (self.tmp / 'workflow.json').write_text(json.dumps(workflow_record()))
        # The GitHub release assets qualified_artifacts.py checked; publish re-checks all of them after approval.
        self.release = self.tmp / 'release'
        self.release.mkdir()
        (self.release / rb.RECEIPT).write_text('{"stand-in": "private qualification receipt"}\n')
        (self.release / rb.SUMS).write_text('stand-in checksums\n')

    def release_outputs(self):
        # Recomputed independently: the text publish-pypi.yml rebuilds in shell after approval.
        text = ''.join(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n'
                       for path in sorted(self.release.iterdir(), key=lambda path: path.name.encode()))
        return {'release_inventory_sha256': hashlib.sha256(text.encode()).hexdigest()}

    def set_version(self, version):
        (self.root / 'python/pyproject.toml').write_text(f'[project]\nname = "eigentrail"\nversion = "{version}"\n')

    def create_command(self):
        subprocess.run([sys.executable, '-I', self.script, 'create', *self.common, '--candidates', str(self.candidates),
                        '--out', str(self.bundle), '--workflow-ref', WORKFLOW_REF, '--ref', REF,
                        '--run-attempt', str(ATTEMPT)], check=True, capture_output=True)
        return {name: rb.sha256(self.bundle / name) for name in (SDIST, WHEEL, LINUX_WHEEL, WINDOWS_WHEEL)}

    def verify_command(self, approved, dist, *extra):
        return subprocess.run([sys.executable, '-I', self.script, 'verify', *self.common, '--bundle', str(self.bundle),
                               '--run', str(self.tmp / 'run.json'), '--workflow', str(self.tmp / 'workflow.json'),
                               '--approved', str(approved), '--release', str(self.release), '--tag', 'v' + VERSION,
                               '--dist', str(dist), *extra],
                              capture_output=True, text=True)


class CommandLineTests(CommandLineFixture):
    def test_create_then_verify_writes_github_outputs(self):
        hashes = self.create_command()
        approved = self.tmp / 'approved.json'
        approved.write_text(json.dumps(approval(hashes)))
        output = self.tmp / 'github-output'
        result = self.verify_command(approved, self.tmp / 'dist', '--github-output', str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        values = dict(line.split('=', 1) for line in output.read_text().splitlines())
        self.assertEqual(values, {'sdist': SDIST, 'sdist_sha256': hashes[SDIST],
                                  'wheels': ','.join(sorted((WHEEL, LINUX_WHEEL, WINDOWS_WHEEL))),
                                  'wheel_sha256s': ','.join(hashes[name] for name in sorted((WHEEL, LINUX_WHEEL, WINDOWS_WHEEL))),
                                  **self.release_outputs(),
                                  'validation_run_attempt': str(ATTEMPT)})
        self.assertEqual({p.name for p in (self.tmp / 'dist').iterdir()},
                         {SDIST, WHEEL, LINUX_WHEEL, WINDOWS_WHEEL})
        # The version must come from --root (the candidate checkout), not this script's own checkout.
        self.set_version('0.2.0')
        failed = self.verify_command(approved, self.tmp / 'dist2')
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn('does not name version 0.2.0', failed.stderr)
        self.assertFalse((self.tmp / 'dist2').exists())

    def test_rejects_approval_with_duplicate_keys(self):
        hashes = self.create_command()
        approved = self.tmp / 'approved.json'
        approved.write_text(json.dumps(approval(hashes)).replace('{', '{"schema": "other", ', 1))
        failed = self.verify_command(approved, self.tmp / 'dist')
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn('Duplicate key in approval', failed.stderr)
        self.assertFalse((self.tmp / 'dist').exists())

    def test_requires_the_release_receipt_and_checksums_publish_re_checks(self):
        hashes = self.create_command()
        approved = self.tmp / 'approved.json'
        approved.write_text(json.dumps(approval(hashes)))
        for name in (rb.RECEIPT, rb.SUMS):
            with self.subTest(missing=name):
                data = (self.release / name).read_bytes()
                (self.release / name).unlink()
                failed = self.verify_command(approved, self.tmp / 'dist')
                self.assertNotEqual(failed.returncode, 0)
                self.assertIn(f'has no regular {name}', failed.stderr)
                self.assertFalse((self.tmp / 'dist').exists())
                (self.release / name).write_bytes(data)

    def test_release_inventory_changes_with_any_asset(self):
        (self.release / WHEEL).write_bytes(b'release wheel')
        (self.release / '-').write_bytes(b'')  # A literal file, which sha256sum would read as stdin.
        original = rb.release_inventory(self.release)
        self.assertEqual(rb.inventory_sha256(original), self.release_outputs()['release_inventory_sha256'])
        changes = [('asset bytes', lambda: (self.release / WHEEL).write_bytes(b'other wheel')),
                   ('asset named -', lambda: (self.release / '-').write_bytes(b'not empty')),
                   ('asset added', lambda: (self.release / 'notes.txt').write_text('added\n')),
                   ('asset removed', lambda: (self.release / WHEEL).unlink())]
        for label, change in changes:
            with self.subTest(change=label):
                backup = self.tmp / 'release-backup'
                shutil.copytree(self.release, backup)
                change()
                self.assertNotEqual(rb.release_inventory(self.release), original)
                shutil.rmtree(self.release)
                backup.rename(self.release)
        self.assertEqual(rb.release_inventory(self.release), original)
        for bad in ['bad name.whl', 'tab\tname']:
            with self.subTest(bad=bad):
                (self.release / bad).write_bytes(b'x')
                self.rejects('Unsafe GitHub release asset name', rb.release_inventory, self.release)
                (self.release / bad).unlink()
        (self.release / 'folder').mkdir()
        self.rejects('only regular files', rb.release_inventory, self.release)


class ReleaseApprovalTests(CommandLineFixture):
    """Run the real qualified_artifacts.py on a GitHub release made from the bundle's files."""

    def setUp(self):
        super().setUp()
        self.hashes = self.create_command()
        for name in (SDIST, WHEEL, LINUX_WHEEL, WINDOWS_WHEEL):
            shutil.copyfile(self.bundle / name, self.release / name)
        (self.release / R_SOURCE).write_bytes(b'R source package')

    def seal(self):
        """Write the private qualification receipt and checksums for the release's current files."""
        approved = {'qualification': 'passed', 'publication': 'approved', 'candidate_only': False,
                    'qualification_receipt_sha256': 'd' * 64}
        targets = {
            WHEEL: (MAC_TAG, {'os': 'macos', 'architecture': 'arm64', 'abi': 'cp314', 'minimum_cpu': 'arm64',
                              'numerical_backend': 'inari-directed-v1'}),
            LINUX_WHEEL: (LINUX_TAG, {'os': 'linux', 'architecture': 'x86_64', 'abi': 'cp314',
                                      'minimum_cpu': 'x86-64-v1', 'numerical_backend': 'inari-r-backport-sse2'}),
            WINDOWS_WHEEL: (WINDOWS_TAG, {'os': 'windows', 'architecture': 'x86_64', 'abi': 'cp314',
                                          'minimum_cpu': 'haswell+os-avx', 'numerical_backend': 'inari-r-backport-avx-fma'}),
        }
        artifacts = {SDIST: dict(approved), R_SOURCE: {'qualification': 'passed'}}
        for name, (tag, target) in targets.items():
            artifacts[name] = dict(approved, wheel_tag=tag, target=target)
        for name, entry in artifacts.items():
            entry['sha256'] = rb.sha256(self.release / name)
        receipt = {'commit': COMMIT, 'tree': TREE, 'pypi_ready': True, 'qualification': {'status': 'passed'},
                   'artifacts': artifacts,
                   'python_distribution': {'schema': rb.APPROVAL_SCHEMA,
                                           'wheel_tags': [MAC_TAG, LINUX_TAG, WINDOWS_TAG], 'sdist': SDIST}}
        (self.release / 'release-receipt.json').write_text(json.dumps(receipt))
        (self.release / 'SHA256SUMS').write_text(''.join(f'{entry["sha256"]}  {name}\n'
                                                         for name, entry in artifacts.items()))

    def approve(self):
        """Run qualified_artifacts.py as publish-pypi.yml does; it must accept the release on its own."""
        result = subprocess.run([sys.executable, '-I', str(QUALIFIED_ARTIFACTS), '--directory', str(self.release),
                                 '--project', str(self.root / 'python/pyproject.toml'), '--commit', COMMIT,
                                 '--tree', TREE, '--out', str(self.tmp / 'approved')],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        path = self.tmp / 'approved.json'
        path.write_text(result.stdout)
        return path

    def test_release_receipt_for_the_bundle_bytes_is_accepted(self):
        self.seal()
        output = self.tmp / 'github-output'
        result = self.verify_command(self.approve(), self.tmp / 'dist', '--github-output', str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({p.name: rb.sha256(p) for p in (self.tmp / 'dist').iterdir()}, self.hashes)
        values = dict(line.split('=', 1) for line in output.read_text().splitlines())
        self.assertEqual({key: values[key] for key in self.release_outputs()}, self.release_outputs())

    def test_release_receipt_for_a_rebuilt_wheel_is_refused(self):
        # As for 0.2.0: the private qualification attached a wheel it built itself, not CI's bytes.
        write_wheel(self.release / WHEEL, [ARM64_TAG], b'Generator: local rebuild\n')
        self.seal()
        result = self.verify_command(self.approve(), self.tmp / 'dist')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('release receipt approves', result.stderr)
        self.assertFalse((self.tmp / 'dist').exists())


def workflow_inventory_block():
    """The publish job's post-approval inventory check, exactly as publish-pypi.yml runs it."""
    lines = PUBLISH_WORKFLOW.read_text().splitlines()
    start = next(i for i, line in enumerate(lines) if line.strip().startswith('inventory=$('))
    end = next(i for i in range(start, len(lines)) if '"$RELEASE_INVENTORY_SHA256" ]]' in lines[i])
    indent = len(lines[start]) - len(lines[start].lstrip())
    return '\n'.join(line[indent:] for line in lines[start:end + 1]) + '\n'


def workflow_bundle_publish_block():
    """The publisher's post-approval copy, wheel allowlist and hash check."""
    lines = PUBLISH_WORKFLOW.read_text().splitlines()
    step = lines.index('      - name: Download the validation bundle and keep only the verified bytes')
    start = next(i for i in range(step, len(lines)) if lines[i].strip().startswith('[[ "$VALIDATION_RUN_ID"'))
    end = next(i for i in range(start, len(lines))
               if '[[ "$(ls -A dist | LC_ALL=C sort)" == "$expected" ]]' in lines[i])
    indent = len(lines[start]) - len(lines[start].lstrip())
    return '\n'.join(line[indent:] for line in lines[start:end + 1]) + '\n'


@unittest.skipUnless(shutil.which('bash') and shutil.which('sha256sum'),
                     'needs bash and GNU sha256sum, as on the publish runner')
class PublishShellTests(unittest.TestCase):
    """Run the workflow's own inventory shell against release_inventory()'s canonical text."""

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.release = Path(folder.name)
        files = {rb.RECEIPT: b'{"receipt": true}\n', rb.SUMS: b'checksums\n', 'Bravo.whl': b'wheel',
                 'alpha.tar.gz': b'sdist', '.dotfile': b'hidden', '-': b'not empty: sha256sum - would read stdin'}
        for name, data in files.items():
            (self.release / name).write_bytes(data)
        self.expected = rb.inventory_sha256(rb.release_inventory(self.release))

    def run_block(self):
        # A locale that collates differently from bytewise order: the block itself must sort with LC_ALL=C.
        env = {'PATH': os.environ['PATH'], 'LANG': 'en_US.UTF-8', 'RELEASE_INVENTORY_SHA256': self.expected}
        return subprocess.run(['bash', '--noprofile', '--norc', '-eo', 'pipefail', '-c', workflow_inventory_block()],
                              cwd=self.release, env=env, capture_output=True, text=True)

    def test_shell_inventory_matches_release_inventory(self):
        result = self.run_block()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_shell_inventory_refuses_a_changed_dash_asset(self):
        (self.release / '-').write_bytes(b'changed while approval was pending')
        self.assertNotEqual(self.run_block().returncode, 0)

    def test_shell_inventory_refuses_added_or_removed_assets(self):
        (self.release / 'extra.txt').write_bytes(b'added')
        self.assertNotEqual(self.run_block().returncode, 0)
        (self.release / 'extra.txt').unlink()
        (self.release / 'Bravo.whl').unlink()
        self.assertNotEqual(self.run_block().returncode, 0)


@unittest.skipUnless(shutil.which('bash') and shutil.which('sha256sum'),
                     'needs bash and GNU sha256sum, as on the publish runner')
class PublishWheelTests(unittest.TestCase):
    """Execute the publisher's exact wheel/hash block against synthetic bundle files."""

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.tmp = Path(folder.name)
        self.bundle = self.tmp / 'source-bundle'
        self.bundle.mkdir()
        self.names = sorted((SDIST, WHEEL, LINUX_WHEEL, WINDOWS_WHEEL))
        for name in self.names:
            (self.bundle / name).write_bytes(('artifact:' + name).encode())
        self.hashes = {name: rb.sha256(self.bundle / name) for name in self.names}
        self.bindir = self.tmp / 'bin'
        self.bindir.mkdir()
        gh = self.bindir / 'gh'
        gh.write_text('''#!/bin/bash
set -euo pipefail
if [[ "$1" == api ]]; then printf '%s\\n' "$RUN_RECORD"; exit 0; fi
if [[ "$1" == run && "$2" == download ]]; then
  while (($#)); do if [[ "$1" == --dir ]]; then destination=$2; shift 2; else shift; fi; done
  mkdir -p "$destination"
  cp "$BUNDLE_SOURCE"/* "$destination/"
  exit 0
fi
exit 90
''')
        gh.chmod(0o755)

    def run_block(self, wheel_names=None, wheel_hashes=None):
        wheel_names = wheel_names or [name for name in self.names if name.endswith('.whl')]
        wheel_hashes = wheel_hashes or [self.hashes[name] for name in wheel_names]
        runner = self.tmp / f'runner-{len(list(self.tmp.glob("runner-*")))}'
        runner.mkdir()
        env = {
            'PATH': str(self.bindir) + os.pathsep + os.environ['PATH'],
            'RUNNER_TEMP': str(runner), 'GITHUB_REPOSITORY': REPO,
            'VALIDATION_RUN_ID': str(RUN_ID), 'VALIDATION_RUN_ATTEMPT': str(ATTEMPT),
            'EXPECTED_SHA': COMMIT, 'RUN_RECORD': f'completed success {ATTEMPT} {COMMIT}',
            'SDIST': SDIST, 'SDIST_SHA256': self.hashes[SDIST],
            'WHEELS': ','.join(wheel_names), 'WHEEL_SHA256S': ','.join(wheel_hashes),
            'BUNDLE_SOURCE': str(self.bundle),
        }
        result = subprocess.run(['bash', '--noprofile', '--norc', '-euo', 'pipefail', '-c',
                                 workflow_bundle_publish_block()], cwd=self.tmp, env=env,
                                capture_output=True, text=True)
        return result, runner / 'dist'

    def test_publisher_stages_exact_wheels_for_paired_hashes(self):
        wheels = [name for name in self.names if name.endswith('.whl')]
        hashes = [self.hashes[name] for name in wheels]
        result, dist = self.run_block(wheels[::-1], hashes[::-1])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({path.name: rb.sha256(path) for path in dist.iterdir()}, self.hashes)

    def test_publisher_refuses_swapped_hashes_and_unsupported_tags(self):
        wheels = [name for name in self.names if name.endswith('.whl')]
        hashes = [self.hashes[name] for name in wheels]
        self.assertNotEqual(self.run_block(wheels, hashes[::-1])[0].returncode, 0)
        unsupported = [name.replace('manylinux_2_28', 'manylinux_2_17') for name in wheels]
        self.assertNotEqual(self.run_block(unsupported, hashes)[0].returncode, 0)


# How both publish-pypi.yml jobs peel the release tag, after pointing tag_repo at their own clone.
TAG_PEEL = ('git -C "$tag_repo" fetch --quiet --no-tags origin "refs/tags/$RELEASE_TAG:refs/tags/$RELEASE_TAG"',
            '[[ "$(git -C "$tag_repo" rev-parse --verify --quiet "refs/tags/$RELEASE_TAG^{commit}")" == "$EXPECTED_SHA" ]]')
TAG_REPOS = {'verify': 'tag_repo=candidate', 'publish': 'tag_repo="$RUNNER_TEMP/main-history"'}


def job_lines(job):
    """The stripped lines of one publish-pypi.yml job, from its key to the next job's."""
    lines = PUBLISH_WORKFLOW.read_text().splitlines()
    keys = [i for i, line in enumerate(lines) if re.fullmatch(r'  [a-z][a-z0-9_-]*:', line)]
    start = lines.index(f'  {job}:')
    end = next((i for i in keys if i > start), len(lines))
    return [line.strip() for line in lines[start:end]]


def job_tag_peel(job):
    """The job's one tag_repo assignment and the two peeling lines after it."""
    lines = job_lines(job)
    found = [tuple(lines[i:i + 3]) for i, line in enumerate(lines) if line.startswith('tag_repo=')]
    if len(found) != 1:
        raise AssertionError(f'the {job} job peels the release tag {len(found)} times, not once')
    return found[0]


def job_main_clone(job='publish'):
    """The publish job's bare, treeless clone of main, with its continuation lines."""
    lines = job_lines(job)
    start = end = next(i for i, line in enumerate(lines) if line.startswith('git clone '))
    while lines[end].endswith('\\'):
        end += 1
    return lines[start:end + 1]


def isolated_git_environment(home):
    """Only PATH comes from the caller: an inherited GIT_DIR, GIT_WORK_TREE or global config must
    not point the fixture's commits and tags at a real repository."""
    environment = {'PATH': os.environ.get('PATH', os.defpath), 'HOME': str(home), 'GIT_CONFIG_NOSYSTEM': '1',
                   'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_TERMINAL_PROMPT': '0',
                   'GIT_AUTHOR_NAME': 'tag test', 'GIT_AUTHOR_EMAIL': 'tag-test@example.invalid',
                   'GIT_COMMITTER_NAME': 'tag test', 'GIT_COMMITTER_EMAIL': 'tag-test@example.invalid'}
    if 'SYSTEMROOT' in os.environ:
        environment['SYSTEMROOT'] = os.environ['SYSTEMROOT']
    return environment


@unittest.skipUnless(shutil.which('git') and shutil.which('bash'), 'needs git and bash')
class TagPeelTests(unittest.TestCase):
    """Each job peels the release tag once, identically, through any depth of annotated tags."""

    DEPTH = 24  # Deeper than the 16-level API walk the publish job used to stop at.

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.tmp = Path(folder.name)
        self.env = isolated_git_environment(self.tmp)
        self.origin = self.tmp / 'origin'
        self.make_origin(self.origin)
        self.runs = 0

    def git(self, *args, env=None):
        return subprocess.run(['git', *args], check=True, capture_output=True, text=True,
                              env=env or self.env).stdout.strip()

    def make_origin(self, path, env=None):
        """main holds the expected commit; an unmerged branch holds another; deep tag chains reach both."""
        def git(*args):
            return self.git('-C', str(path), *args, env=env)
        self.git('init', '-q', '-b', 'main', str(path), env=env)
        git('config', 'uploadpack.allowFilter', 'true')  # Serve the publish job's treeless clone as GitHub does.
        git('commit', '-q', '--allow-empty', '-m', 'expected')
        self.expected = git('rev-parse', 'HEAD')
        git('switch', '-q', '-c', 'unmerged')
        git('commit', '-q', '--allow-empty', '-m', 'off main')
        self.off_main = git('rev-parse', 'HEAD')
        git('switch', '-q', 'main')
        git('commit', '-q', '--allow-empty', '-m', 'later on main')
        for name, target in (('v1.0.0', self.expected), ('v2.0.0', self.off_main)):
            previous = target
            for level in range(self.DEPTH - 1):
                git('tag', '-a', '-m', f'{name} level {level}', f'{name}-{level}', previous)
                previous = f'{name}-{level}'
            git('tag', '-a', '-m', name, name, previous)
        git('tag', '-a', '-m', 'not a commit', 'v3.0.0', git('rev-parse', 'HEAD^{tree}'))

    def run_job(self, job, tag):
        """Run the job's own lines (for publish, its clone of main from GitHub, served here) and its peel."""
        self.runs += 1
        work = self.tmp / f'{job}-{self.runs}'
        temp = work / 'runner-temp'
        temp.mkdir(parents=True)
        lines = []
        if job == 'verify':  # actions/checkout left the candidate clone, with an origin remote.
            clone = work / 'candidate'
            self.git('init', '-q', str(clone))
            self.git('-C', str(clone), 'remote', 'add', 'origin', self.origin.as_uri())
        else:
            clone = temp / 'main-history'
            lines += job_main_clone()
        lines += job_tag_peel(job)
        env = {**self.env, 'RELEASE_TAG': tag, 'EXPECTED_SHA': self.expected, 'RUNNER_TEMP': str(temp),
               'GITHUB_REPOSITORY': REPO, 'GIT_CONFIG_COUNT': '1',
               'GIT_CONFIG_KEY_0': f'url.{self.origin.as_uri()}.insteadOf',
               'GIT_CONFIG_VALUE_0': f'https://github.com/{REPO}.git'}
        result = subprocess.run(['bash', '--noprofile', '--norc', '-eo', 'pipefail', '-c', '\n'.join(lines) + '\n'],
                                cwd=work, env=env, capture_output=True, text=True)
        return result, clone

    def test_each_job_peels_once_with_the_same_commands(self):
        for job, assignment in TAG_REPOS.items():
            with self.subTest(job=job):
                self.assertEqual(job_tag_peel(job), (assignment, *TAG_PEEL))

    def test_deep_chain_to_the_expected_commit_is_accepted_by_both_jobs(self):
        for job in TAG_REPOS:
            with self.subTest(job=job):
                result, _ = self.run_job(job, 'v1.0.0')
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_deep_chain_to_an_off_main_commit_is_fetched_then_refused(self):
        for job in TAG_REPOS:
            with self.subTest(job=job):
                result, clone = self.run_job(job, 'v2.0.0')
                self.assertNotEqual(result.returncode, 0)
                # The fetch worked, also into the publish job's treeless partial clone: the comparison refused it.
                self.assertEqual(self.git('-C', str(clone), 'rev-parse', 'refs/tags/v2.0.0^{commit}'), self.off_main)

    def test_non_commit_and_missing_tags_are_refused_by_both_jobs(self):
        for job in TAG_REPOS:
            for tag in ('v3.0.0', 'v9.9.9'):  # A tag of a tree; no such tag.
                with self.subTest(job=job, tag=tag):
                    self.assertNotEqual(self.run_job(job, tag)[0].returncode, 0)

    def test_fixture_git_ignores_an_inherited_git_dir(self):
        decoy = self.tmp / 'decoy'
        self.git('init', '-q', '-b', 'main', str(decoy))
        self.git('-C', str(decoy), 'commit', '-q', '--allow-empty', '-m', 'decoy')
        state = lambda: (self.git('-C', str(decoy), 'rev-parse', 'HEAD'), self.git('-C', str(decoy), 'tag', '--list'))
        before = state()
        with mock.patch.dict(os.environ, {'GIT_DIR': str(decoy / '.git'), 'GIT_WORK_TREE': str(decoy)}):
            self.make_origin(self.tmp / 'second-origin', env=isolated_git_environment(self.tmp))
        self.assertEqual(state(), before)

if __name__ == '__main__':
    unittest.main()
