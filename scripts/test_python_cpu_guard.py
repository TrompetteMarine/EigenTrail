"""Check Linux import refusal before any native extension code can run."""
from pathlib import Path
import builtins
import platform
import sys
import unittest
from unittest.mock import mock_open, patch

SOURCE = Path(__file__).resolve().parents[1] / 'python/eigentrail/__init__.py'
CODE = compile(SOURCE.read_text(), str(SOURCE), 'exec')
FLAGS = 'avx avx2 bmi1 bmi2 cx16 f16c fma fxsr abm movbe pclmulqdq popcnt rdrand sse sse2 pni sse4_1 sse4_2 ssse3 xsave xsaveopt'
IMPORT = builtins.__import__


class NativeImportReached(Exception):
    pass


def import_gate(name, *args, **kwargs):
    if name == '_eigentrail':
        raise NativeImportReached
    return IMPORT(name, *args, **kwargs)


def probe(cpuinfo, machine='x86_64', system='linux', error=None):
    with (patch.object(sys, 'platform', system),
          patch.object(platform, 'machine', return_value=machine),
          patch('builtins.open', mock_open(read_data=cpuinfo)) as opened,
          patch('builtins.__import__', side_effect=import_gate)):
        if error is not None:
            opened.side_effect = error
        exec(CODE, {'__name__': 'eigentrail', '__package__': 'eigentrail'})


class CpuGuardTests(unittest.TestCase):
    def test_supported_and_nonlinux_reach_native_import(self):
        for system, machine, cpuinfo in [('linux', 'x86_64', 'flags : ' + FLAGS),
                                        ('darwin', 'arm64', '')]:
            with self.subTest(system=system), self.assertRaises(NativeImportReached):
                probe(cpuinfo, machine, system)

    def test_unsupported_hosts_stop_before_native_import(self):
        for cpuinfo, machine in [('', 'x86_64'), ('flags : sse sse2', 'x86_64'),
                                 ('flags : ' + FLAGS, 'aarch64'),
                                 ('flags : ' + FLAGS + '\nflags : ' + FLAGS.replace('avx ', ''), 'x86_64')]:
            with self.subTest(machine=machine, cpuinfo=cpuinfo), self.assertRaisesRegex(ImportError, 'EigenTrail'):
                probe(cpuinfo, machine)
        with self.assertRaisesRegex(ImportError, 'readable /proc/cpuinfo'):
            probe('', error=OSError('unavailable'))


if __name__ == '__main__':
    unittest.main()
