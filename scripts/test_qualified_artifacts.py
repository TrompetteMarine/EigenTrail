"""Synthetic selector controls only; these fixtures are not native qualification."""
from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from zipfile import ZipFile

from qualified_artifacts import SCHEMA, digest, select
from check_release import CLASSIFIERS, CONTACT, REPOSITORY, core_metadata

COMMIT, TREE = '1' * 40, '2' * 40
TAG = 'cp314-cp314-macosx_14_0_arm64'
WHEEL = 'eigentrail-0.3.0-' + TAG + '.whl'
SDIST = 'eigentrail-0.3.0.tar.gz'


def wheel(path, tag=TAG, name='eigentrail'):
    with ZipFile(path, 'w') as archive:
        archive.writestr('eigentrail-0.3.0.dist-info/WHEEL',
            'Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: ' + tag + '\n')
        archive.writestr('eigentrail-0.3.0.dist-info/METADATA',
            'Metadata-Version: 2.4\nName: ' + name + '\nVersion: 0.3.0\n')


def entry():
    return {'qualification': 'passed', 'publication': 'approved', 'candidate_only': False,
            'qualification_receipt_sha256': 'a' * 64}


def seal(root, receipt):
    for name, info in receipt['artifacts'].items():
        info['sha256'] = digest(root / name)
    (root / 'release-receipt.json').write_text(json.dumps(receipt))
    (root / 'SHA256SUMS').write_text(''.join(info['sha256'] + '  ' + name + '\n'
        for name, info in receipt['artifacts'].items()))


class SelectorTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.folder = Path(self.temporary.name)
        self.root = self.folder / 'qualified'; self.root.mkdir()
        self.project = self.folder / 'pyproject.toml'
        self.project.write_text('[project]\nname="eigentrail"\nversion="0.3.0"\n')
        wheel(self.root / WHEEL); (self.root / SDIST).write_bytes(b'synthetic source archive')
        artifact = dict(entry(), wheel_tag=TAG, target={'os': 'macos', 'architecture': 'arm64',
            'abi': 'cp314', 'minimum_cpu': 'arm64', 'numerical_backend': 'inari-directed-v1'})
        self.receipt = {'commit': COMMIT, 'tree': TREE, 'pypi_ready': True,
            'qualification': {'status': 'passed'}, 'artifacts': {WHEEL: artifact, SDIST: entry()},
            'python_distribution': {'schema': SCHEMA, 'wheel_tags': [TAG], 'sdist': SDIST}}
        seal(self.root, self.receipt)

    def selected(self):
        return select(self.root, self.project, COMMIT, TREE)

    def reject(self, mutate):
        mutate(self.receipt); seal(self.root, self.receipt)
        with self.assertRaises((ValueError, KeyError, TypeError)):
            self.selected()

    def test_exact_selection_and_fresh_copy(self):
        self.assertEqual([p.name for p in self.selected()], [WHEEL, SDIST])
        output = self.folder / 'dist'
        command = [sys.executable, str(Path(__file__).with_name('qualified_artifacts.py')),
            '--directory', str(self.root), '--project', str(self.project), '--commit', COMMIT,
            '--tree', TREE, '--out', str(output)]
        subprocess.run(command, check=True, capture_output=True)
        self.assertEqual({p.name for p in output.iterdir()}, {WHEEL, SDIST})
        for path in output.iterdir(): self.assertEqual(digest(path), digest(self.root / path.name))
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)

    def test_release_binding_and_readiness(self):
        for change in [('commit', '3' * 40), ('tree', '4' * 40), ('pypi_ready', False)]:
            with self.subTest(change=change):
                copy = deepcopy(self.receipt); copy[change[0]] = change[1]; seal(self.root, copy)
                with self.assertRaises(ValueError): self.selected()
        self.reject(lambda r: r['qualification'].update(status='pending'))

    def test_legacy_receipt_is_not_reinterpreted(self):
        self.reject(lambda r: r.pop('python_distribution'))

    def test_missing_extra_duplicate_and_wrong_abi_wheels(self):
        cases = [('missing', 'cp314-cp314-win_amd64'), ('duplicate', TAG), ('wrong', 'cp313-cp313-win_amd64')]
        for kind, tag in cases:
            with self.subTest(kind=kind):
                copy = deepcopy(self.receipt)
                copy['python_distribution']['wheel_tags'].append(tag)
                seal(self.root, copy)
                with self.assertRaises(ValueError): self.selected()
        extra = 'eigentrail-0.3.0-1-' + TAG + '.whl'; wheel(self.root / extra)
        self.reject(lambda r: r['artifacts'].update({extra: deepcopy(r['artifacts'][WHEEL])}))

    def test_unlisted_wheel_and_checksum_corruption(self):
        extra = self.root / 'extra.whl'; extra.write_bytes(b'not listed')
        with self.assertRaises(ValueError): self.selected()
        extra.unlink(); (self.root / SDIST).write_bytes(b'changed bytes')
        with self.assertRaises(ValueError): self.selected()

    def test_internal_wheel_tags_are_checked_after_hashes(self):
        # Letters and a backslash at either end must not be stripped as if they were blanks.
        for tag in ('cp313-cp313-macosx_14_0_arm64', TAG + '\u00a0', '\u00a0' + TAG, TAG + '\u3000',
                    't' + TAG, TAG + 'n', TAG + 'r', TAG + '\\', '\\' + TAG):
            with self.subTest(tag=tag):
                wheel(self.root / WHEEL, tag); seal(self.root, self.receipt)
                with self.assertRaises(ValueError): self.selected()
        for tag in (TAG + ' ', TAG + '\t', ' \t' + TAG):  # ASCII blanks around a header value are tolerated.
            with self.subTest(tag=tag):
                wheel(self.root / WHEEL, tag); seal(self.root, self.receipt)
                self.assertEqual(len(self.selected()), 2)

    def test_internal_package_name_is_checked_exactly(self):
        for name in ('eigentrailn', 'neigentrail', 'eigentrailt', 'reigentrail', 'eigentrail\\', 'eigentrail\u00a0'):
            with self.subTest(name=name):
                wheel(self.root / WHEEL, name=name); seal(self.root, self.receipt)
                with self.assertRaises(ValueError): self.selected()

    def test_unqualified_candidate_and_tuple_tampering(self):
        changes = [('qualification', 'pending'), ('candidate_only', True), ('publication', 'held'),
                   ('qualification_receipt_sha256', 'missing'), ('wheel_tag', 'cp314-cp314-win_amd64')]
        for field, value in changes:
            with self.subTest(field=field):
                copy = deepcopy(self.receipt); copy['artifacts'][WHEEL][field] = value; seal(self.root, copy)
                with self.assertRaises(ValueError): self.selected()
        for field, value in [('os', 'windows'), ('architecture', 'x86_64'), ('abi', 'abi3'),
                             ('minimum_cpu', 'unknown'), ('numerical_backend', 'unqualified')]:
            with self.subTest(field=field):
                copy = deepcopy(self.receipt); copy['artifacts'][WHEEL]['target'][field] = value; seal(self.root, copy)
                with self.assertRaises(ValueError): self.selected()

    def test_linux_v3_generic_publication_is_held_even_with_approved_labels(self):
        name = 'eigentrail-0.3.0-cp314-cp314-manylinux_2_28_x86_64.whl'
        wheel(self.root / name, 'cp314-cp314-manylinux_2_28_x86_64')
        (self.root / WHEEL).unlink()
        artifact = self.receipt['artifacts'].pop(WHEEL)
        artifact.update(wheel_tag='cp314-cp314-manylinux_2_28_x86_64',
            target=dict(os='linux', architecture='x86_64', abi='cp314', minimum_cpu='x86-64-v3+os-avx', numerical_backend='inari-directed-v1'))
        self.receipt['artifacts'][name] = artifact
        self.receipt['python_distribution']['wheel_tags'] = [artifact['wheel_tag']]
        seal(self.root, self.receipt)
        with self.assertRaises(ValueError): self.selected()
        artifact['target']['minimum_cpu'] = 'x86-64-v1'; seal(self.root, self.receipt)
        self.assertEqual({p.name for p in self.selected()}, {name, SDIST})

    def test_exactly_one_sdist_and_duplicate_receipt_keys(self):
        source = 'eigentrail-0.3.0.zip'; (self.root / source).write_bytes(b'other source')
        self.reject(lambda r: r['artifacts'].update({source: entry()}))
        (self.root / source).unlink(); seal(self.root, {**self.receipt, 'artifacts': {k: v for k, v in self.receipt['artifacts'].items() if k != source}})
        path = self.root / 'release-receipt.json'
        path.write_text(path.read_text().replace('"pypi_ready": true', '"pypi_ready": true,"pypi_ready": true'))
        with self.assertRaises(ValueError): self.selected()


class MetadataTests(unittest.TestCase):
    def setUp(self):
        self.project = {'name': 'eigentrail', 'version': '0.3.0', 'description': 'Synthetic metadata',
            'dependencies': ['numpy>=2.3,<3'], 'optional-dependencies': {'plot': ['matplotlib>=3.10,<4']},
            'readme': {'text': 'Synthetic controls only.', 'content-type': 'text/markdown'},
            'urls': {'Homepage': REPOSITORY}}
        self.text = '\n'.join(['Name: eigentrail', 'Version: 0.3.0', 'Summary: Synthetic metadata',
            'Maintainer-email: Gabriel Bontemps <' + CONTACT + '>', 'License-Expression: MIT',
            'Description-Content-Type: text/markdown', 'Requires-Python: >=3.14,<3.15',
            *('Classifier: ' + value for value in sorted(CLASSIFIERS)), 'Project-URL: Homepage, ' + REPOSITORY,
            'Requires-Dist: numpy (<3,>=2.3)', 'Provides-Extra: plot',
            'Requires-Dist: matplotlib (<4,>=3.10); extra == "plot"', '', 'Synthetic controls only.'])

    def test_optional_plot_is_separate_from_the_base_numpy_dependency(self):
        core_metadata(self.text.encode(), self.project)
        core_metadata(self.text.replace('extra == "plot"', "extra == 'plot'").encode(), self.project)

    def test_missing_extra_wrong_bounds_and_accidental_base_dependency_fail(self):
        for altered in [self.text.replace('Provides-Extra: plot\n', ''),
                        self.text.replace('<4,>=3.10', '<5,>=3.10'),
                        self.text.replace('; extra == "plot"', ''),
                        self.text.replace('extra == "plot"', 'extra == "other"'),
                        self.text.replace('numpy (<3,>=2.3)', 'numpy (<3,>=2.4)')]:
            with self.subTest(altered=altered):
                with self.assertRaises(ValueError): core_metadata(altered.encode(), self.project)


if __name__ == '__main__':
    unittest.main()
