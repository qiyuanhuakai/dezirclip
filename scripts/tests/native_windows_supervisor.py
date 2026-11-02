"""Bound only the spawned functional controller; never infer product cleanup from its exit."""
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import time
import uuid


CONTROLLER_SECONDS = 600
KILL_WAIT_SECONDS = 5
RECEIPT_NAME = 'controller-supervisor.json'


def write_supervisor_receipt(output_root, receipt):
    """Publish new metadata only after the controller created its ordinary output directory."""
    root = Path(output_root).absolute()
    try:
        root.lstat()
    except FileNotFoundError:
        return False
    for component in (root, *root.parents):
        info = component.lstat()
        if not stat.S_ISDIR(info.st_mode) or stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError('Supervisor output directory must have no links or reparse points')
    destination = root / RECEIPT_NAME
    temporary = root / ('.controller-supervisor-' + uuid.uuid4().hex + '.tmp')
    try:
        with temporary.open('x', encoding='utf-8') as stream:
            json.dump(receipt, stream, indent=2)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        # An exclusive hard link publishes a complete file without replacing any original.
        os.link(temporary, destination)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
    return True


def run_controller(command, *, cwd, output_root, popen=None, clock=None, write_receipt=None):
    """Preserve a returned nonzero code; a total deadline is 124, including unconfirmed teardown."""
    popen = subprocess.Popen if popen is None else popen
    clock = time.monotonic if clock is None else clock
    write_receipt = write_supervisor_receipt if write_receipt is None else write_receipt
    started = clock()
    child = None
    receipt = {'schema_version': 1, 'kind': 'windows_native_functional_supervisor',
               'command': list(command), 'controller_deadline_seconds': CONTROLLER_SECONDS,
               'kill_wait_seconds': KILL_WAIT_SECONDS, 'child_started': False, 'pid': None,
               'controller_exit_code': None, 'timed_out': False, 'kill_requested': False,
               'kill_call_returned': False, 'termination_confirmed': False,
               'termination_unconfirmed': False, 'first_failure': None, 'secondary_errors': [],
               'product_cleanup': 'not_verified_by_supervisor'}
    exit_code = 1

    def retain_failure(stage, error):
        item = {'stage': stage, 'error': str(error)}
        if receipt['first_failure'] is None:
            receipt['first_failure'] = item
        else:
            receipt['secondary_errors'].append(item)

    def observed_exit(code):
        if not isinstance(code, int) or isinstance(code, bool):
            raise ValueError('Controller wait must return an integer exit code')
        receipt['controller_exit_code'] = code
        receipt['termination_confirmed'] = True
        return code

    try:
        # Inherit streams: no reader threads, communicate(), shell, or process-tree termination.
        child = popen(list(command), cwd=cwd)
        receipt['child_started'] = True
        receipt['pid'] = child.pid
        try:
            remaining = max(0.0, CONTROLLER_SECONDS - (clock() - started))
            exit_code = observed_exit(child.wait(timeout=remaining))
            if exit_code != 0:
                retain_failure('controller_exit', 'Controller returned ' + str(exit_code))
            elif clock() - started > CONTROLLER_SECONDS:
                receipt['timed_out'] = True
                exit_code = 124
                retain_failure('controller_deadline', 'Controller exceeded its total 600 second deadline')
        except subprocess.TimeoutExpired:
            receipt['timed_out'] = True
            exit_code = 124
            retain_failure('controller_deadline', 'Controller exceeded its total 600 second deadline')
        except BaseException as error:
            exit_code = 1
            retain_failure('controller_wait', error)
        if not receipt['termination_confirmed']:
            receipt['kill_requested'] = True
            try:
                child.kill()
                receipt['kill_call_returned'] = True
            except BaseException as error:
                retain_failure('controller_kill', error)
            try:
                observed_exit(child.wait(timeout=KILL_WAIT_SECONDS))
            except BaseException as error:
                retain_failure('controller_kill_wait', error)
                receipt['termination_unconfirmed'] = True
    except BaseException as error:
        retain_failure('controller_start', error)
    receipt['elapsed_seconds'] = clock() - started
    receipt['supervisor_exit_code'] = exit_code
    try:
        if write_receipt(output_root, receipt) is False:
            raise ValueError('Controller output directory is absent; supervisor did not create it')
    except BaseException as error:
        retain_failure('supervisor_receipt', error)
        if exit_code == 0:
            exit_code = 1
        receipt['supervisor_exit_code'] = exit_code
        print('Controller supervisor receipt was not published: ' + json.dumps(receipt), file=sys.stderr)
    return exit_code
