"""Small synthetic rolling smoke for explicit installed CLI/Python runtimes."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import struct
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def canonical(value):
    if isinstance(value, float):
        return 'f64:' + struct.pack('>d', value).hex()
    if isinstance(value, list):
        return [canonical(item) for item in value]
    if isinstance(value, dict):
        return {key: canonical(item) for key, item in value.items()}
    return value


def text(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False)


def request(tolerance, tied=False):
    matrices = [[4., 0., 0., 1.], [1., 0., 0., 1.] if tied else [8., 0., 0., 2.]]
    return canonical({
        'schema': 'eigentrail.rolling-lineage.v1',
        'chart': {'frame': 'public-synthetic', 'coordinates': 'frame_coefficients'},
        'features': [{'id': 'x', 'unit': 'unit'}, {'id': 'y', 'unit': 'unit'}],
        'dates': [{'id': 'd0', 'time': 0.}, {'id': 'd1', 'time': 1.}],
        'subjects': [{'id': 'subject', 'slots': [
            {'tag': 'observed', 'covariance': {'n': 2, 'data': entries}}
            for entries in matrices]}],
        'lenses': ['covariance'],
        'representation': {'symmetry': 'exact', 'correction': {'kind': 'none'}},
        'grouping': {'rtol': 0.}, 'tolerance_deg': float(tolerance),
        'uncertainty': {'mode': 'point'},
        'scope': {'lens_travel': [['covariance', 'covariance']],
                  'rank_masks': [{'lens': 'covariance', 'ranks': [[0, 2]]}],
                  'max_lag': 1, 'missing': 'skip_recorded',
                  'policy': 'draft_f_conservative',
                  'certificate': 'observed_dated_subspaces', 'direct_scope': 'all_later_slots'},
        'founders': [{'subject': 0, 'date': 0, 'lens': 'covariance', 'rank': [0, 1],
                      'policies': ['resolved_line', 'typed_block_graph']}],
        'projection': {'kind': 'full'},
        'limits': {'max_candidates': 64, 'max_edge_records': 64,
                   'max_bytes': 32 * 1024 * 1024, 'chunk_records': 64},
        'precision': {'format': 'binary64', 'angle_unit': 'degrees'}})


def records(report, kind):
    for chunk in report['chunks']:
        if chunk['records']['kind'] == kind:
            yield from chunk['records']['records']


def validate(report, name):
    assert report['schema'] == 'eigentrail.rolling-lineage.v1'
    assert report['completion']['status'] == 'complete'
    counts = report['completion']['graph_counts']
    expected = {'parallel_zero': (4, 2, 2), 'parallel_ninety': (4, 4, 0),
                'tie': (2, 2, 0)}[name]
    assert (counts['candidates'], counts['agreement'], counts['exclusion']) == expected
    assert counts['open'] == counts['unavailable'] == 0
    if name == 'tie':
        target = next(view for view in records(report, 'views') if view['date'] == 1)
        assert len(target['groups']) == 1
        group = target['groups'][0]
        assert group['rank'] == [0, 2] and group['information'] == 'whole_space'
        assert group['multiplicity'] == {'kind': 'exact', 'count': 2}
        founder = next(row for row in records(report, 'founders') if row['kind'] == 'target')
        line = next(row for row in founder['families'] if row['family'] == 'line_only')
        assert line['reach'] == {'kind': 'unavailable', 'reason': 'unresolved_internal_line'}


PROBE = r'''
import hashlib,json,pathlib,sys
import eigentrail
from eigentrail import _eigentrail
folder=pathlib.Path(sys.argv[1]);source=pathlib.Path(sys.argv[2])
package=pathlib.Path(eigentrail.__file__).resolve()
native=pathlib.Path(_eigentrail.__file__).resolve()
assert not package.is_relative_to(source) and not native.is_relative_to(source)
prefix=pathlib.Path(sys.prefix).resolve()
assert package.is_relative_to(prefix) and native.is_relative_to(prefix)
native_hash=hashlib.sha256(native.read_bytes()).hexdigest()
request=(folder/'request.json').read_text()
report=eigentrail.analyze_rolling_lineage_json(request)
assert eigentrail.check_rolling_lineage_json(request,report) is True
(folder/'python-report.json').write_text(report)
bad=json.loads(report);bad['completion']['graph_counts']['agreement']+=1
bad=json.dumps(bad,sort_keys=True,separators=(',',':'))
try:eigentrail.check_rolling_lineage_json(request,bad)
except ValueError:pass
else:raise AssertionError('accepted altered aggregate count')
assert hashlib.sha256(native.read_bytes()).hexdigest()==native_hash
(folder/'python-paths.json').write_text(json.dumps({'python':sys.executable,
 'version':sys.version,'package':str(package),'native':str(native),
 'native_sha256':native_hash}))
'''


def run(argv, folder, success=True):
    result = subprocess.run([str(arg) for arg in argv], cwd=folder,
                            capture_output=True, text=True, timeout=300)
    assert (result.returncode == 0) == success, (argv, result.returncode, result.stderr)
    return {'argv': [str(arg) for arg in argv], 'exit_code': result.returncode,
            'stdout': result.stdout, 'stderr': result.stderr}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True, help='fresh directory outside checkout')
    parser.add_argument('--cli', type=Path, help='exact installed eigentrail-check executable')
    parser.add_argument('--python', type=Path, help='exact installed-package Python interpreter')
    args = parser.parse_args()
    if args.cli is None and args.python is None:
        parser.error('supply --cli and/or --python explicitly')
    out = args.out.resolve()
    assert not out.is_relative_to(ROOT), 'run outside the source checkout'
    out.mkdir(parents=True, exist_ok=False)
    cli = args.cli.resolve() if args.cli else None
    python = args.python.absolute() if args.python else None
    if cli:
        assert cli.is_file() and not cli.is_relative_to(ROOT)
    cli_hash = hashlib.sha256(cli.read_bytes()).hexdigest() if cli else None
    commands, hashes = [], {}
    python_binding = None
    for name, tolerance, tied in [('parallel_zero', 0., False),
                                   ('parallel_ninety', 90., False), ('tie', 0., True)]:
        folder = out / name
        folder.mkdir()
        (folder / 'request.json').write_text(text(request(tolerance, tied)))
        if cli:
            commands.append(run([cli, 'analyze-rolling-lineage', folder/'request.json', folder/'cli-report.json'], folder))
            report_text = (folder/'cli-report.json').read_text()
            validate(json.loads(report_text), name)
            commands.append(run([cli, 'check-rolling-lineage', folder/'request.json', folder/'cli-report.json'], folder))
            altered = json.loads(report_text)
            altered['completion']['graph_counts']['agreement'] += 1
            (folder/'tampered.json').write_text(text(altered))
            commands.append(run([cli, 'check-rolling-lineage', folder/'request.json', folder/'tampered.json'], folder, False))
            commands.append(run([cli, 'analyze-rolling-lineage', folder/'request.json', folder/'cli-report.json'], folder, False))
        if python:
            commands.append(run([python, '-I', '-c', PROBE, folder, ROOT], folder))
            validate(json.loads((folder/'python-report.json').read_text()), name)
            binding = json.loads((folder/'python-paths.json').read_text())
            assert python_binding is None or binding == python_binding
            python_binding = binding
            if cli:
                assert (folder/'python-report.json').read_bytes() == (folder/'cli-report.json').read_bytes()
        for path in folder.iterdir():
            if path.is_file():
                hashes[path.relative_to(out).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    assert not cli or hashlib.sha256(cli.read_bytes()).hexdigest() == cli_hash
    receipt = {'schema': 'eigentrail.rolling-public-smoke.v1', 'status': 'passed',
               'scope': 'Small synthetic installed-interface smoke; not full scientific or platform qualification',
               'platform': platform.platform(), 'machine': platform.machine(),
               'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'cli_sha256': cli_hash, 'python_binding': python_binding,
               'files': hashes, 'commands': commands}
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print('Rolling synthetic installed smoke passed:', out)


if __name__ == '__main__':
    main()
