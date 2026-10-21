"""Two actual processes exercise ownership only; no clipboard is opened."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    fixture = args.fixture.resolve(strict=True)
    directory = Path(tempfile.mkdtemp(prefix='dzc-capture-lease-'))
    variants = [({'DISPLAY': ':99'}, 'x11-base'), ({'DISPLAY': ':99.0'}, 'x11-screen-alias'),
                ({'WAYLAND_DISPLAY': 'wayland-0'}, 'wayland-relative'),
                ({'WAYLAND_DISPLAY': str(directory/'wayland-0')}, 'wayland-absolute'),
                ({'WAYLAND_DISPLAY': str(directory/'alias')}, 'wayland-alias-label')]
    report = {'kind': 'actual-cross-process-capture-lease', 'timing_eligible': False,
              'status': 'running', 'cases': [], 'profile': str(directory)}

    def environment(values):
        result = dict(os.environ)
        for key in ('DISPLAY','WAYLAND_DISPLAY','XDG_SESSION_TYPE'):
            result.pop(key, None)
        result.update(values)
        return result

    output = directory/'holder.log'
    with output.open('wb') as log:
        holder = subprocess.Popen([str(fixture),'--capture-lease-fixture'], env=environment(variants[0][0]),
                                  stdin=subprocess.PIPE, stdout=log, stderr=log)
        try:
            deadline = time.monotonic()+5
            while b'capture-lease-acquired' not in output.read_bytes():
                assert holder.poll() is None, 'holder failed before readiness'
                assert time.monotonic() < deadline, 'holder readiness timed out'
                time.sleep(.01)
            for values, label in variants:
                result = subprocess.run([str(fixture),'--capture-lease-check'], env=environment(values),
                                        capture_output=True, timeout=5)
                report['cases'].append({'scope':label,'phase':'held','exit_code':result.returncode,
                                        'stdout':result.stdout.decode('utf-8'),'stderr':result.stderr.decode('utf-8')})
                assert result.returncode == 3 and b'capture-lease-busy' in result.stderr, report['cases'][-1]
            holder.communicate(b'\n', timeout=5)
            assert holder.returncode == 0
            for values, label in variants:
                result = subprocess.run([str(fixture),'--capture-lease-check'], env=environment(values),
                                        capture_output=True, timeout=5)
                report['cases'].append({'scope':label,'phase':'released','exit_code':result.returncode,
                                        'stdout':result.stdout.decode('utf-8'),'stderr':result.stderr.decode('utf-8')})
                assert result.returncode == 0 and b'capture-lease-acquired' in result.stdout, report['cases'][-1]
            report['status'] = 'passed'
        except Exception as error:
            report['status'] = 'failed'
            report['failure'] = repr(error)
            raise
        finally:
            if holder.poll() is None:
                holder.kill()
                holder.wait(timeout=5)
            if args.report:
                args.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    if report['status'] == 'passed':
        shutil.rmtree(directory)
    print('Actual process-held capture lease rejects display aliases and becomes available after exit.')


if __name__ == '__main__':
    main()
