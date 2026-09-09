import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile

from android_robot_device import device_lock


def digest(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def write_report(path, report):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as output:
        json.dump(report, output, indent=2)
        output.write('\n')
        output.flush()
        os.fsync(output.fileno())
    os.replace(output.name, path)


def checked_command(command, *, cwd=None, env=None, timeout=60, output=None):
    with subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, start_new_session=True) as process:
        try:
            data, diagnostics = process.communicate(timeout=timeout)
        except BaseException:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                data, diagnostics = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                data, diagnostics = process.communicate(timeout=5)
            if output:
                Path(output).write_bytes(diagnostics + data)
            raise
    if output:
        Path(output).write_bytes(diagnostics + data)
    if process.returncode:
        raise subprocess.CalledProcessError(process.returncode, command, output=diagnostics + data)
    return data


def error_details(error):
    result = {'type': type(error).__name__, 'message': str(error)}
    if isinstance(error, BaseExceptionGroup):
        result['children'] = [error_details(child) for child in error.exceptions]
    if isinstance(error, (subprocess.CalledProcessError, subprocess.TimeoutExpired)):
        result['command'] = error.cmd
        output = error.output
        result['output'] = output.decode(errors='replace') if isinstance(output, bytes) else output
    return result


def run_reported(path, report, work, cleanup=()):
    report.update(status='running', acceptance_eligible=False)
    write_report(path, report)
    errors = []
    try:
        work()
    except BaseException as error:
        errors.append(error)
        report['invalid_reason'] = f'{type(error).__name__}: {error}'
    finally:
        eligible = report.get('acceptance_eligible', False)
        report['acceptance_eligible'] = False
        try:
            write_report(path, report)
        except BaseException as error:
            errors.append(error)
            report['report_error'] = str(error)
        for finish in cleanup:
            try:
                finish()
            except BaseException as error:
                errors.append(error)
                report.setdefault('cleanup_errors', []).append(f'{type(error).__name__}: {error}')
        report['status'] = 'failed' if errors else 'complete'
        report['acceptance_eligible'] = eligible and not errors
        if errors:
            report['acceptance_eligible'] = False
            report.setdefault('invalid_reason', 'cleanup failed')
            report['errors'] = [error_details(error) for error in errors]
        try:
            write_report(path, report)
        except BaseException as error:
            errors.append(error)
    if errors:
        raise BaseExceptionGroup('Benchmark failed; evidence retained in ' + str(path), errors)


def interrupted(number, _frame):
    raise KeyboardInterrupt(f'Signal {number}')
