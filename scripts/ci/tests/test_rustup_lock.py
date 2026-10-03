import os
from pathlib import Path
import subprocess
import tempfile
import unittest


WRAPPER = Path(__file__).resolve().parents[1] / "rustup_locked.sh"


class RustupProcesses(unittest.TestCase):
    def test_shared_installations_serialize_and_preserve_failures(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            rustup = root / "rustup"
            rustup.write_text(
                "#!/usr/bin/env python3\n"
                "import os, pathlib, sys, time\n"
                "guard = pathlib.Path(os.environ['RUSTUP_HOME']) / 'active'\n"
                "guard.mkdir()\n"
                "try:\n"
                "    time.sleep(0.15)\n"
                "finally:\n"
                "    guard.rmdir()\n"
                "sys.exit(int(sys.argv[1]))\n"
            )
            rustup.chmod(0o755)
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}",
                       RUSTUP_HOME=str(root / "toolchains"))
            children = []
            try:
                for status in (0, 37, 0, 0):
                    children.append((status, subprocess.Popen(
                        ["bash", str(WRAPPER), str(status)], env=env,
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                    )))
                for status, child in children:
                    output, _ = child.communicate(timeout=10)
                    self.assertEqual(child.returncode, status, output)
            finally:
                for _, child in children:
                    if child.poll() is None:
                        child.kill()
                    child.communicate()


if __name__ == "__main__":
    unittest.main()
