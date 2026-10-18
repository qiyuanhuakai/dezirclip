#!/usr/bin/env python3
"""Run the packaged CLI in clean Ubuntu 22.04 and Debian 12 userspaces."""

import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
IMAGES = ("ubuntu:22.04", "debian:bookworm-slim")


def verify(archive, version=None):
    archive = archive.resolve()
    if not archive.name.endswith(("-linux-x86_64.tar.gz", "-linux-aarch64.tar.gz")):
        raise ValueError("expected a Linux standalone dzc archive")
    script = r'''
set -eu
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 libdbus-1-3 libgcc-s1 >/dev/null
getconf GNU_LIBC_VERSION
python3 /source/scripts/verify-dzc-package.py "/artifacts/$DZC_ARCHIVE" ${DZC_VERSION:+--version "$DZC_VERSION"}
python3 - <<'PY'
import importlib.util
import os
from pathlib import Path
spec = importlib.util.spec_from_file_location("verify_dzc", "/source/scripts/verify-dzc-package.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.extract_archive(Path("/artifacts") / os.environ["DZC_ARCHIVE"], Path("/tmp/dzc-smoke"))
PY
ldd /tmp/dzc-smoke/dzc
DZC_TEST_BIN=/tmp/dzc-smoke/dzc LC_ALL=C.UTF-8 python3 -m unittest discover -s /source/integrations/tests -p test_native_bash_pty.py -v
'''
    for image in IMAGES:
        print(f"Verifying actual standalone archive in {image}", flush=True)
        subprocess.run(["docker", "run", "--rm", "-v", f"{ROOT}:/source:ro",
                        "-v", f"{archive.parent}:/artifacts:ro",
                        "-e", f"DZC_ARCHIVE={archive.name}", "-e", f"DZC_VERSION={version or ''}",
                        image, "bash", "-c", script], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--version")
    args = parser.parse_args()
    try:
        if os.name != "posix":
            raise ValueError("run this gate on a native Linux runner (or inside WSL)")
        verify(args.archive, args.version)
        return 0
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"dzc old-userspace verification: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
