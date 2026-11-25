"""Synthetic pre-import Windows guard tests; no Windows DLL execution on other OSes."""
import ast
import ctypes
from pathlib import Path
import platform
import struct
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[1]


def guard_source():
    source = ROOT / 'python/eigentrail/__init__.py'
    if not source.is_file():
        source = ROOT.parents[1] / 'python/eigentrail/__init__.py'
    tree = ast.parse(source.read_text())
    guards = [node for node in tree.body if isinstance(node, ast.If)
              and ast.unparse(node.test) == "sys.platform == 'win32'"]
    assert len(guards) == 1
    native = next(node for node in tree.body if isinstance(node, ast.ImportFrom)
                  and node.module == '_eigentrail')
    assert tree.body.index(guards[0]) < tree.body.index(native)
    return compile(ast.Module(body=guards, type_ignores=[]), str(source), 'exec')


class WindowsGuardTests(unittest.TestCase):
    def probe(self, *, present=True, status=0, load_error=None, machine='AMD64', pointer_size=8):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / '__init__.py'
            helper = source.with_name('_eigentrail_cpu_guard.dll')
            if present:
                helper.write_bytes(b'synthetic-placeholder-not-a-dll')
            function = Mock(return_value=status)
            library = types.SimpleNamespace(eigentrail_windows_haswell_guard=function)
            with (patch.object(sys, 'platform', 'win32'),
                  patch.object(platform, 'machine', return_value=machine),
                  patch.object(struct, 'calcsize', return_value=pointer_size),
                  patch.object(ctypes, 'CDLL', return_value=library, side_effect=load_error) as load):
                namespace = {'sys': sys, 'platform': platform, 'struct': struct, '__file__': str(source)}
                exec(guard_source(), namespace)
                load.assert_called_once_with(str(helper.resolve()), winmode=0x1100)
                self.assertEqual(function.argtypes, [])
                self.assertIs(function.restype, ctypes.c_uint32)
                function.assert_called_once_with()

    def test_verified_guard_precedes_extension_import(self):
        self.probe()

    def test_missing_dll_and_loader_failure_refuse(self):
        with self.assertRaisesRegex(ImportError, 'baseline Windows CPU guard'):
            self.probe(present=False)
        with self.assertRaisesRegex(ImportError, 'could not run'):
            self.probe(load_error=OSError('missing dependent DLL'))

    def test_every_failure_status_and_unknown_status_refuse(self):
        for status in [1, 2, 3, 4, 5, 0xffffffff]:
            with self.subTest(status=status), self.assertRaisesRegex(ImportError, 'refused status'):
                self.probe(status=status)

    def test_wrong_process_architecture_refuses(self):
        for machine, size in [('ARM64', 8), ('AMD64', 4)]:
            with self.subTest(machine=machine, size=size), self.assertRaisesRegex(ImportError, 'x86-64 CPython'):
                self.probe(machine=machine, pointer_size=size)


if __name__ == '__main__':
    unittest.main()
