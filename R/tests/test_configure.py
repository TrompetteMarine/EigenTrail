"""Exercise configure with mock tool versions; never invoke Rust or a build."""
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


PACKAGE = Path(__file__).resolve().parents[1]


class ConfigureTests(unittest.TestCase):
    def run_configure(self, system_version, personal_version=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package, tools, home = root / 'package', root / 'bin', root / 'home'
            (package / 'src/rust/.cargo').mkdir(parents=True)
            tools.mkdir()
            (home / '.cargo/bin').mkdir(parents=True)
            for name in ('awk', 'sed', 'uname', 'tar', 'xz'):
                executable = shutil.which(name)
                if executable:
                    (tools / name).symlink_to(executable)
            for name in ('cargo', 'rustc'):
                for folder, version in ((tools, system_version), (home / '.cargo/bin', personal_version)):
                    if version is not None:
                        tool = folder / name
                        tool.write_text('#!/bin/sh\n[ "$1" = --version ] || exit 90\nprintf "' + name + ' ' + version + ' (mock)\\n"\n')
                        tool.chmod(0o755)
            shutil.copyfile(PACKAGE / 'src/Makevars.in', package / 'src/Makevars.in')
            (package / 'src/rust/.cargo/config.toml').write_text('[net]\noffline=true\n')
            with tarfile.open(package / 'src/rust/vendor.tar.xz', 'w:xz') as archive:
                member = tarfile.TarInfo('vendor/example/source.rs')
                member.size = 6
                archive.addfile(member, io.BytesIO(b'source'))
            environment = dict(os.environ, PATH=str(tools), HOME=str(home))
            for variable in ('CARGO', 'RUSTC', 'RUSTUP_TOOLCHAIN'):
                environment.pop(variable, None)
            result = subprocess.run(['/bin/sh', str(PACKAGE / 'configure')], cwd=package,
                                    env=environment, text=True, capture_output=True)
            generated = package / 'src/Makevars'
            return result, generated.read_text() if generated.exists() else '', home

    def test_current_tools_generate_offline_bounded_build(self):
        result, makevars, _ = self.run_configure('1.99.0')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Using rustc 1.99.0', result.stderr)
        self.assertIn('--locked --offline -j 2', makevars)
        self.assertNotIn('@CARGO@', makevars)

    def test_old_system_tools_fall_back_to_suitable_personal_tools(self):
        result, makevars, home = self.run_configure('1.90.0', '1.99.0')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(home / '.cargo/bin/cargo'), makevars)

    def test_old_or_missing_tools_refuse_without_build(self):
        for version in ('1.90.0', None):
            result, makevars, _ = self.run_configure(version)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('requires installed cargo >= 1.99.0', result.stderr)
            self.assertEqual(makevars, '')


if __name__ == '__main__':
    unittest.main()
