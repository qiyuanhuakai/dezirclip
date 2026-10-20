"""Run the Rust/Go native transport fixture in a new private directory."""
import argparse
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-fixture", type=Path, required=True)
    parser.add_argument("--go", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    environment = {**os.environ, "GOTOOLCHAIN": "local"}
    with tempfile.TemporaryDirectory(prefix="dzc-wire-") as directory:
        Path(directory).chmod(0o700)
        for language in ("go", "rust"):
            server = subprocess.Popen([str(args.rust_fixture.resolve()), directory], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            line = queue.Queue(maxsize=1)
            threading.Thread(target=lambda: line.put(server.stdout.readline()), daemon=True).start()
            try:
                address = line.get(timeout=10).strip()
                if not address:
                    raise RuntimeError("fixture did not announce its endpoint")
                command = ([str(args.go.resolve()), "run", "./cmd/protocol-fixture", address] if language == "go"
                           else [str(args.rust_fixture.resolve()), "--identity-client", directory, str(server.pid)])
                subprocess.run(command, cwd=root / "clients/go", env=environment, check=True, timeout=40)
                if server.wait(timeout=10) != 0:
                    raise RuntimeError("fixture failed: " + server.stderr.read())
            finally:
                if server.poll() is None:
                    server.terminate()
                    try:
                        server.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        server.kill()
                        server.wait()


if __name__ == "__main__":
    main()
