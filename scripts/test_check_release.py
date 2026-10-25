"""Independent metadata/archive boundary checks; no builds or uploads."""
from pathlib import Path
import io
import tarfile
import tempfile
import unittest
import zipfile

from check_release import check_release

PROJECT='''[project]
name="eigentrail"
version="0.1.0"
description="Checked covariance trajectories"
requires-python=">=3.14,<3.15"
dependencies=["numpy>=2.3,<3"]
license="MIT"
license-files=["LICENSE"]
authors=[{name="Gabriel Bontemps",email="gbontemps@protonmail.com"}]
maintainers=[{name="Gabriel Bontemps",email="gbontemps@protonmail.com"}]
classifiers=["Programming Language :: Python :: 3.14","Programming Language :: Python :: Implementation :: CPython","Operating System :: MacOS :: MacOS X"]
[project.readme]
content-type="text/markdown"
text="Synthetic long description."
[project.urls]
Homepage="https://github.com/TrompetteMarine/EigenTrail"
Source="https://github.com/TrompetteMarine/EigenTrail"
Documentation="https://github.com/TrompetteMarine/EigenTrail/blob/main/docs/quickstart.md"
Issues="https://github.com/TrompetteMarine/EigenTrail/issues"
'''

METADATA=b'''Metadata-Version: 2.4
Name: eigentrail
Version: 0.1.0
Summary: Checked covariance trajectories
Requires-Python: <3.15,>=3.14
Requires-Dist: numpy<3,>=2.3
License-Expression: MIT
Maintainer-email: Gabriel Bontemps <gbontemps@protonmail.com>
Description-Content-Type: text/markdown
Classifier: Programming Language :: Python :: 3.14
Classifier: Programming Language :: Python :: Implementation :: CPython
Classifier: Operating System :: MacOS :: MacOS X
Project-URL: Homepage, https://github.com/TrompetteMarine/EigenTrail
Project-URL: Source, https://github.com/TrompetteMarine/EigenTrail
Project-URL: Documentation, https://github.com/TrompetteMarine/EigenTrail/blob/main/docs/quickstart.md
Project-URL: Issues, https://github.com/TrompetteMarine/EigenTrail/issues

Synthetic long description.
'''


class ReleaseMetadata(unittest.TestCase):
    def setUp(self):
        self.temporary=tempfile.TemporaryDirectory(prefix='eigentrail-release-metadata-')
        self.addCleanup(self.temporary.cleanup)
        self.root=Path(self.temporary.name)
        (self.root/'python').mkdir();(self.root/'R').mkdir()
        (self.root/'python/pyproject.toml').write_text(PROJECT)
        (self.root/'python/LICENSE').write_text('MIT License\n')
        (self.root/'Cargo.toml').write_text('''[workspace.package]
version="0.1.0"
authors=["Gabriel Bontemps <gbontemps@protonmail.com>"]
repository="https://github.com/TrompetteMarine/EigenTrail"
''')
        (self.root/'R/DESCRIPTION').write_text('''Package: eigentrail
Version: 0.1.0
Authors@R: person("Gabriel", "Bontemps", email="gbontemps@protonmail.com", role=c("aut","cre"))
''')

    def wheel(self,metadata=METADATA):
        path=self.root/'eigentrail-0.1.0-py3-none-any.whl'
        with zipfile.ZipFile(path,'w') as archive:
            archive.writestr('eigentrail-0.1.0.dist-info/METADATA',metadata)
        return path

    def sdist(self,project=PROJECT):
        path=self.root/'eigentrail-0.1.0.tar.gz'
        with tarfile.open(path,'w:gz') as archive:
            for name,data in [('pyproject.toml',project.encode()),('PKG-INFO',METADATA)]:
                info=tarfile.TarInfo('eigentrail-0.1.0/'+name);info.size=len(data)
                archive.addfile(info,io.BytesIO(data))
        return path

    def test_sources_and_relocated_sdist_metadata_pass_without_claiming_publication(self):
        result=check_release(self.root,[self.wheel(),self.sdist()])
        self.assertEqual(result['status'],'passed')
        self.assertEqual(len(result['artifacts']),2)
        self.assertIs(result['published'],False)
        self.assertIs(result['release_qualified'],False)

    def test_placeholder_contacts_and_mismatched_versions_are_rejected(self):
        path=self.root/'python/pyproject.toml'
        path.write_text(PROJECT.replace('gbontemps@protonmail.com','gabriel@example.invalid'))
        with self.assertRaises(ValueError):check_release(self.root)
        path.write_text(PROJECT)
        path=self.root/'R/DESCRIPTION';path.write_text(path.read_text().replace('Version: 0.1.0','Version: 0.2.0'))
        with self.assertRaises(ValueError):check_release(self.root)

    def test_readme_cannot_depend_on_checkout_parent(self):
        path=self.root/'python/pyproject.toml'
        path.write_text(PROJECT.replace('text="Synthetic long description."','file="../README.md"'))
        with self.assertRaises(ValueError):check_release(self.root)

    def test_archive_metadata_must_keep_contact_and_long_description(self):
        bad=self.wheel(METADATA.replace(b'gbontemps@protonmail.com',b'gabriel@example.invalid'))
        with self.assertRaises(ValueError):check_release(self.root,[bad])
        bad=self.wheel(METADATA.replace(b'Synthetic long description.\n',b''))
        with self.assertRaises(ValueError):check_release(self.root,[bad])
        bad=self.sdist(PROJECT.replace('text="Synthetic long description."','file="../README.md"'))
        with self.assertRaises(ValueError):check_release(self.root,[bad])

    def test_placeholder_urls_and_ambiguous_metadata_are_rejected(self):
        path=self.root/'python/pyproject.toml'
        path.write_text(PROJECT.replace('https://github.com/TrompetteMarine/EigenTrail/issues','https://example.invalid/issues'))
        with self.assertRaises(ValueError):check_release(self.root)
        path.write_text(PROJECT)
        wheel=self.wheel()
        with zipfile.ZipFile(wheel,'a') as archive:
            archive.writestr('other-0.1.0.dist-info/METADATA',METADATA)
        with self.assertRaises(ValueError):check_release(self.root,[wheel])


if __name__=='__main__':unittest.main()
