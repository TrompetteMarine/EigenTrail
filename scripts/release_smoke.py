"""Exercise an installed package and CLI on one independent constant-path fixture."""
from fractions import Fraction
from pathlib import Path
import argparse
import hashlib
import json
import os
import platform
import shutil
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
RADIUS = 2.0**-20


def wire(value):
    if isinstance(value, float): return 'f64:' + struct.pack('>d', value).hex()
    if isinstance(value, dict): return {key: wire(item) for key, item in value.items()}
    if isinstance(value, list): return [wire(item) for item in value]
    return value


def decoded(value):
    if isinstance(value, str) and value.startswith('f64:'): return struct.unpack('>d', bytes.fromhex(value[4:]))[0]
    if isinstance(value, dict): return {key: decoded(item) for key, item in value.items()}
    if isinstance(value, list): return [decoded(item) for item in value]
    return value


def fixture():
    return wire({'schema': 'eigentrail.trace.v1', 'times': [0., 1.],
        'covariances': [{'n': 2, 'data': [.75, 0., 0., .25]} for _ in range(2)],
        'features': ['feature_0', 'feature_1'], 'lipschitz': 0.,
        'calibration': {'kind': 'Conditional', 'radii': [RADIUS, RADIUS],
            'assumptions': 'supplied nodal covariance envelope; declared global Lipschitz regularity'},
        'correction': [0., 0.], 'target': .002, 'numerical_allowance': .00001, 'memory_bytes': 512 * 1024**2})


def validate_report(report):
    report = decoded(report)
    assert report['schema'] == 'eigentrail.trace.v1' and report['completed'] is True
    assert len(report['eigensystems']) == 2 and len(report['segments']) == 1
    for eigensystem in report['eigensystems']:
        assert len(eigensystem['values']) == 2
        assert all(abs(value - truth) < 1e-12 for value, truth in zip(eigensystem['values'], [.75, .25]))
    segment = report['segments'][0]
    assert segment['association'] == {'kind': 'Unique', 'matching': [0, 1]}
    assert [(tube['start'], tube['end']) for tube in segment['tubes']] == [(0, 1), (1, 2)]
    radius = Fraction(1, 2**20)
    # The observed gap is exactly1/2; Weyl charges one radius per endpoint eigenvalue.
    exact_bound = 2 * radius / (Fraction(1, 2) - 2 * radius)
    for tube in segment['tubes']:
        assert tube['informative'] is True and exact_bound <= Fraction.from_float(tube['bound']) < Fraction(1, 100000)
    assert len(report['summaries']) == 2
    for summaries in report['summaries']:
        assert len(summaries) == 2
        for summary, truth, rank in zip(summaries, [.75, .25], [1, 2]):
            interval = summary['strength']
            assert interval['lower'] <= truth <= interval['upper']
            assert summary['rank'] == [rank, rank]
            assert summary['share']['kind'] == 'Interval'
            share = summary['share']['value']; assert share['lower'] <= truth <= share['upper']


def command(argv, cwd):
    result = subprocess.run(argv, cwd=cwd, capture_output=True, text=True)
    return {'argv': [str(item) for item in argv], 'exit_code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr}


def environment(expected_machine=None):
    machine = platform.machine().lower()
    if expected_machine: assert machine == expected_machine.lower(), (machine, expected_machine)
    commands = [['uname', '-a'], ['rustc', '-Vv'], ['cargo', '--version'], ['cc', '--version'], ['c++', '--version'], ['R', '--version']]
    if sys.platform == 'darwin': commands += [['sw_vers'], ['sysctl', '-n', 'machdep.cpu.brand_string'], ['sysctl', '-in', 'sysctl.proc_translated']]
    elif sys.platform.startswith('linux'): commands += [['lscpu', '--json']]
    records = []
    for argv in commands:
        if shutil.which(argv[0]): records.append(command(argv, ROOT))
    if sys.platform == 'darwin':
        assert all(record['stdout'].strip() != '1' for record in records if record['argv'][-1] == 'sysctl.proc_translated'), 'Rosetta runner is not native ARM64'
    flags = None
    if sys.platform.startswith('linux') and expected_machine == 'x86_64':
        flags = set(next(line.split(':', 1)[1].split() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('flags')))
        assert {'avx', 'avx2', 'fma', 'bmi1', 'bmi2', 'f16c', 'sse4_2', 'popcnt', 'xsave'} <= flags
        assert os.environ.get('RUSTFLAGS') == '-C target-cpu=haswell'
        for key in ['CFLAGS', 'CXXFLAGS']: assert '-march=haswell' in os.environ.get(key, '')
    return {'platform': platform.platform(), 'machine': machine, 'python': sys.version,
        'commands': records, 'cpu_flags': sorted(flags) if flags else None,
        'build_flags': {key: os.environ.get(key) for key in ['RUSTFLAGS', 'CFLAGS', 'CXXFLAGS', 'PKG_CFLAGS', 'PKG_CXXFLAGS', 'MACOSX_DEPLOYMENT_TARGET']},
        'scope': 'Actual runner and explicit compiler flags; a newer CPU is not execution on an actual Haswell processor.'}


R_PROBE = '''args <- commandArgs(TRUE);folder <- normalizePath(args[[1]]);lib <- normalizePath(args[[2]])
.libPaths(c(lib,.Library));library(eigentrail,lib.loc=lib)
read <- function(name) rawToChar(readBin(file.path(folder,name),"raw",n=file.info(file.path(folder,name))$size))
request <- read("request.json");report <- track_json(request);stopifnot(check_json(request,report))
stopifnot(identical(report,read("report.json")))
stopifnot(inherits(try(check_json(request,read("tampered.json")),silent=TRUE),"try-error"))
stack <- array(rep(c(.75,0,0,.25),2),dim=c(2L,2L,2L))
stopifnot(identical(as.character(track(stack,c(0,1),rep(2^-20,2),0)),report))
package <- normalizePath(system.file(package="eigentrail"));stopifnot(identical(package,normalizePath(file.path(lib,"eigentrail"))))
dlls <- vapply(getLoadedDLLs(),function(info) info[["path"]],"")
selected <- dlls[startsWith(dlls,paste0(package,.Platform$file.sep))];stopifnot(length(selected)==1)
writeLines(c(paste("package",package,sep="\\t"),paste("dll",selected,sep="\\t")),file.path(folder,"r-paths.tsv"))
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--cli', type=Path)
    parser.add_argument('--r-library', type=Path)
    parser.add_argument('--rscript', default='Rscript')
    parser.add_argument('--environment-only', action='store_true')
    parser.add_argument('--expected-machine')
    args = parser.parse_args(); folder = args.out.resolve(); folder.mkdir(parents=True, exist_ok=False)
    if args.environment_only:
        (folder / 'runner.json').write_text(json.dumps(environment(args.expected_machine), indent=2) + '\n')
        return
    assert not folder.is_relative_to(ROOT), 'Installed smoke must run outside checkout'
    os.chdir(folder)
    import eigentrail
    from eigentrail import _eigentrail
    import numpy as np
    paths = {'python': sys.executable, 'package': eigentrail.__file__, 'native': _eigentrail.__file__, 'numpy': np.__file__}
    for key in ['package', 'native', 'numpy']:
        path = Path(paths[key]).resolve(); assert path.is_relative_to(Path(sys.prefix).resolve()) and not path.is_relative_to(ROOT)
    request = json.dumps(fixture(), sort_keys=True, separators=(',', ':'))
    output = eigentrail.track_json(request); report = json.loads(output)
    validate_report(report); assert eigentrail.check_json(request, output) is True
    stack = np.zeros((2, 4, 4), dtype=np.float64)[:, ::2, ::2]; stack[:] = [[.75, 0.], [0., .25]]
    assert not stack.flags.c_contiguous
    assert eigentrail.track_covariances(stack, np.array([0., 1.]), np.array([RADIUS, RADIUS]), 0.) == output
    bad = dict(report, completed=False); tampered = json.dumps(bad, sort_keys=True, separators=(',', ':'))
    try: eigentrail.check_json(request, tampered)
    except ValueError: pass
    else: raise AssertionError('Python accepted incomplete report')
    for name, data in [('request.json', request), ('report.json', output), ('tampered.json', tampered)]:
        (folder / name).write_text(data)
    commands = []
    if args.cli:
        cli = args.cli.resolve(); assert cli.is_file() and not cli.is_relative_to(ROOT)
        for argv, code in [([str(cli), 'track', str(folder / 'request.json'), str(folder / 'cli-report.json')], 0),
                           ([str(cli), 'check', str(folder / 'request.json'), str(folder / 'report.json')], 0),
                           ([str(cli), 'check', str(folder / 'request.json'), str(folder / 'tampered.json')], 1)]:
            result = command(argv, folder); commands.append(result); assert result['exit_code'] == code, result
        assert (folder / 'cli-report.json').read_bytes() == (folder / 'report.json').read_bytes()
    if args.r_library:
        library = args.r_library.resolve(); assert library.is_dir() and not library.is_relative_to(ROOT)
        (folder / 'probe.R').write_text(R_PROBE)
        result = command([args.rscript, '--vanilla', str(folder / 'probe.R'), str(folder), str(library)], folder)
        commands.append(result); assert result['exit_code'] == 0, result
    result = {'status': 'passed', 'scope': 'One conditional constant covariance fixture; no statistical/platform certification.',
        'paths': paths, 'commands': commands, 'canonical_report_sha256': hashlib.sha256(output.encode()).hexdigest()}
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print('Installed constant-path smoke passed:', folder)


if __name__ == '__main__': main()
