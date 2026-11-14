#!/usr/bin/env python3
"""Privately check one frozen R source tarball with an explicit installed R.

No upload or submission is performed. PDF manuals are excluded because this
gate has no TeX installation; native scientific qualification remains separate.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import signal
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def check_status(text):
    summaries = re.findall(r'^Status:\s*(.+)$', text, re.MULTILINE)
    if not summaries:
        return None
    summary = summaries[-1].strip()
    counts = dict(errors=0, warnings=0, notes=0)
    if summary != 'OK':
        matches = re.findall(r'(\d+)\s+(ERROR|WARNING|NOTE)s?\b', summary, re.IGNORECASE)
        remainder = re.sub(r'\d+\s+(?:ERROR|WARNING|NOTE)s?\b', '', summary, flags=re.IGNORECASE)
        if not matches or remainder.strip(' ,'):
            raise ValueError('unrecognized R check status')
        for number, kind in matches:
            counts[kind.lower() + 's'] += int(number)
    return dict(summary=summary, **counts)


def run_process(command, cwd, env, stdout, stderr, timeout):
    started = time.monotonic()
    with stdout.open('xb') as output, stderr.open('xb') as errors:
        process = subprocess.Popen(list(map(str, command)), cwd=cwd, env=env,
                                   stdout=output, stderr=errors, start_new_session=True)
        timed_out = False
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            if os.name == 'posix':
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            code = process.wait()
    return dict(exit_code=code, timed_out=timed_out, seconds=time.monotonic()-started)


def qualify(package, runtime, output, cargo=None, rustc=None):
    package, output = package.resolve(), output.resolve()
    runtime = Path(os.path.abspath(runtime))
    cargo = Path(os.path.abspath(cargo or os.environ.get('CARGO') or shutil.which('cargo') or ''))
    rustc = Path(os.path.abspath(rustc or os.environ.get('RUSTC') or shutil.which('rustc') or ''))
    if not package.is_file() or not all(path.is_file() and os.access(path, os.X_OK)
                                       for path in (runtime, cargo, rustc)):
        raise ValueError('provide an existing tarball and installed executable R/Cargo/rustc paths')
    if output.is_relative_to(ROOT):
        raise ValueError('--out must be private storage outside the source checkout')
    files = {}
    with tarfile.open(package) as archive:
        names = set()
        for member in archive.getmembers():
            path = Path(member.name)
            if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0] != 'eigentrail':
                raise ValueError('unsafe or unexpected package member')
            canonical_name = path.as_posix()
            if canonical_name in names or not (member.isfile() or member.isdir()):
                raise ValueError('duplicate or non-file package member')
            names.add(canonical_name)
            if member.isfile():
                files[member.name] = hashlib.sha256(archive.extractfile(member).read()).hexdigest()
        description = archive.extractfile('eigentrail/DESCRIPTION').read().decode()
    version = re.search(r'^Version:\s*(\S+)\s*$', description, re.MULTILINE)
    if not version or package.name != 'eigentrail_' + version[1] + '.tar.gz':
        raise ValueError('expected the unrenamed R CMD build tarball')
    source_hash = hashlib.sha256(json.dumps(files, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    original_hash = digest(package)
    previous_umask = os.umask(0o077)
    try:
        output.mkdir(parents=True, exist_ok=False)
        logs, inputs, temporary = output / 'logs', output / 'inputs', output / 'tmp'
        for path in (logs, inputs, temporary, output / 'cargo-home', output / 'cargo-target', output / 'library'):
            path.mkdir()
        frozen = inputs / package.name
        shutil.copyfile(package, frozen)
        if digest(frozen) != original_hash:
            raise ValueError('tarball changed while copying')
        substitutions = [(str(package), '$PACKAGE'), (str(output), '$OUTPUT'),
                         (str(runtime), '$R'), (str(cargo), '$CARGO'), (str(rustc), '$RUSTC')]
        def sanitize(text):
            for old, new in sorted(substitutions, key=lambda pair: len(pair[0]), reverse=True):
                text = text.replace(old, new)
            return text
        receipt = dict(schema='eigentrail.cran-qualification.v1', status='running', runner_sha256=digest(__file__),
                       package=dict(name=package.name, version=version[1], sha256=original_hash,
                                    source_manifest_sha256=source_hash, source_files_sha256=files),
                       commands=[], runtime={}, installed_native=[], installed_wrappers={}, submitted=False,
                       scope='R CMD check --as-cran --no-manual on these exact source bytes and runtime',
                       not_checked=['PDF manual (TeX unavailable)', 'X11-dependent functionality',
                                    'CRAN farm execution', 'Full native scientific replay'])
        receipt_path = output / 'receipt.json'
        def save():
            receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
        save()
        env = dict(os.environ, CARGO=str(cargo), RUSTC=str(rustc),
                   CARGO_HOME=str(output / 'cargo-home'), CARGO_TARGET_DIR=str(output / 'cargo-target'),
                   CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0', CARGO_NET_OFFLINE='true',
                   RUSTUP_AUTO_INSTALL='0', MAKEFLAGS='-j2', OPENBLAS_NUM_THREADS='1',
                   OMP_NUM_THREADS='1', TMPDIR=str(temporary), R_LIBS_USER=str(output / 'library'),
                   R_LIBS_SITE='', R_ENVIRON_USER=os.devnull, R_PROFILE_USER=os.devnull,
                   R_MAKEVARS_USER=os.devnull, LC_ALL='en_US.UTF-8' if sys.platform == 'darwin' else 'C')
        env['PATH'] = os.pathsep.join(map(str, (cargo.parent, rustc.parent, runtime.parent))) + os.pathsep + env['PATH']
        started = time.monotonic()
        def run(name, command):
            print(name, flush=True)
            row = dict(name=name, command=[sanitize(str(arg)) for arg in command], completed=False)
            receipt['commands'].append(row)
            save()
            stdout, stderr = logs / (name + '.stdout.log'), logs / (name + '.stderr.log')
            row.update(run_process(command, output, env, stdout, stderr,
                                   max(0.01, 3600 - (time.monotonic()-started))))
            row.update(completed=True, stdout_sha256=digest(stdout), stderr_sha256=digest(stderr))
            save()
            return stdout.read_text(errors='replace'), row
        try:
            for name, command in [('R-version', [runtime, '--version']),
                                  ('cargo-version', [cargo, '--version', '--verbose']),
                                  ('rustc-version', [rustc, '--version', '--verbose']),
                                  ('R-C-compiler-command', [runtime, 'CMD', 'config', 'CC'])]:
                identity, row = run(name, command)
                receipt['runtime'][name] = sanitize(identity.strip())
                if row['exit_code'] != 0 or row['timed_out']:
                    raise RuntimeError(name + ' identity command failed')
            identity, row = run('C-compiler-version', shlex.split(identity.strip()) + ['--version'])
            receipt['runtime']['C-compiler-version'] = sanitize(identity.strip())
            if row['exit_code'] != 0 or row['timed_out']:
                raise RuntimeError('C compiler identity command failed')
            identity, row = run('R-session', [runtime, '--vanilla', '--slave', '-e',
                'cat("version\\t",R.version.string,"\\nplatform\\t",R.version$platform,"\\nmachine\\t",Sys.info()[["machine"]],"\\n",sep="")'])
            receipt['runtime']['R-session'] = sanitize(identity.strip())
            if row['exit_code'] != 0 or row['timed_out']:
                raise RuntimeError('R session identity command failed')
            _, row = run('check', [runtime, 'CMD', 'check', '--as-cran', '--no-manual', frozen])
            check_log = output / 'eigentrail.Rcheck/00check.log'
            receipt['check'] = check_status(check_log.read_text(errors='replace')) if check_log.exists() else None
            installed = output / 'eigentrail.Rcheck/eigentrail'
            native_copies = inputs / 'native'
            native_copies.mkdir()
            for path in sorted((installed / 'libs').rglob('*')):
                if path.is_file() and path.suffix in ('.so', '.dylib', '.dll'):
                    retained = native_copies / path.relative_to(installed / 'libs')
                    retained.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(path, retained)
                    retained.chmod(0o444)
                    if digest(retained) != digest(path):
                        raise RuntimeError('native bytes changed while retaining the checked build')
                    receipt['installed_native'].append(dict(path=str(path.relative_to(output)),
                                                             sha256=digest(path), bytes=path.stat().st_size,
                                                             retained_path=str(retained.relative_to(output)),
                                                             retained_sha256=digest(retained)))
            wrapper_paths = [installed/'NAMESPACE', installed/'DESCRIPTION']
            for folder in ('R', 'Meta'):
                wrapper_paths.extend(path for path in (installed/folder).rglob('*') if path.is_file())
            receipt['installed_wrappers'] = {str(path.relative_to(output)): digest(path)
                                              for path in sorted(wrapper_paths) if path.is_file()}
            receipt['source_unchanged'] = digest(package) == digest(frozen) == original_hash
            checks = receipt['check']
            if row['timed_out'] or row['exit_code'] != 0 or not checks or not receipt['source_unchanged']:
                receipt['status'] = 'failed'
            elif checks['errors'] or checks['warnings'] or not receipt['installed_native']:
                receipt['status'] = 'failed'
            else:
                receipt['status'] = 'needs_review' if checks['notes'] else 'passed'
        except Exception as error:
            receipt.update(status='failed', error=sanitize(type(error).__name__ + ': ' + str(error)))
        receipt['logs'] = {str(path.relative_to(output)): digest(path)
                           for path in sorted(output.rglob('*'))
                           if path.is_file() and path.suffix in ('.log', '.out')}
        receipt['seconds'] = time.monotonic()-started
        save()
        print(json.dumps({key: receipt[key] for key in ('status', 'runtime', 'installed_native', 'seconds')}
                         | {'package_sha256': original_hash, 'check': receipt.get('check')}, indent=2))
        return receipt['status']
    finally:
        os.umask(previous_umask)


def self_test():
    assert check_status('Status: OK\n') == dict(summary='OK', errors=0, warnings=0, notes=0)
    assert check_status('Status: 2 ERRORs, 1 WARNING, 3 NOTEs\n')['notes'] == 3
    assert check_status('* checking installation ... ERROR\n') is None
    try:
        check_status('Status: unexpected\n')
    except ValueError:
        pass
    else:
        raise AssertionError('unknown status accepted')
    with tempfile.TemporaryDirectory(prefix='eigentrail-cran-self-test-') as directory:
        root = Path(directory)
        failed = run_process([sys.executable, '-c', 'raise SystemExit(7)'], root, os.environ,
                             root/'failed.stdout', root/'failed.stderr', 5)
        assert failed['exit_code'] == 7 and not failed['timed_out']
        timed = run_process([sys.executable, '-c', 'import time; time.sleep(10)'], root, os.environ,
                            root/'timeout.stdout', root/'timeout.stderr', 0.1)
        assert timed['timed_out'] and timed['exit_code'] != 0 and timed['seconds'] < 5
        unsafe = root / 'eigentrail_0.0.0.tar.gz'
        with tarfile.open(unsafe, 'w:gz') as archive:
            member = tarfile.TarInfo('../escape')
            member.size = 1
            archive.addfile(member, io.BytesIO(b'x'))
        try:
            qualify(unsafe, Path(sys.executable), root/'unsafe-output',
                    Path(sys.executable), Path(sys.executable))
        except ValueError as error:
            assert 'unsafe' in str(error) and not (root/'unsafe-output').exists()
        else:
            raise AssertionError('unsafe archive accepted')
    print('CRAN qualifier self-test passed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path)
    parser.add_argument('--r', type=Path)
    parser.add_argument('--out', type=Path)
    parser.add_argument('--cargo', type=Path)
    parser.add_argument('--rustc', type=Path)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if not all((args.package, args.r, args.out)):
        parser.error('--package, --r and --out are required')
    status = qualify(args.package, args.r, args.out, args.cargo, args.rustc)
    raise SystemExit(1 if status == 'failed' else 0)


if __name__ == '__main__':
    main()
