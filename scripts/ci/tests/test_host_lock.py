import fcntl
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


WRAPPER = Path(__file__).resolve().parents[1] / "with_host_lock.sh"


class HostLockProcesses(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="cranpose-host-lock-process-")
        self.root = Path(self.scratch.name)
        self.addCleanup(self.scratch.cleanup)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ("bash", "dirname", "date", "python3"):
            (self.bin / name).symlink_to(shutil.which(name))
        self.env = dict(os.environ)
        self.env.pop("CRANPOSE_HOST_LOCK_HELD", None)
        self.env.update(
            PATH=str(self.bin),
            CRANPOSE_USE_SCCACHE="0",
            CRANPOSE_HOST_LOCK_FILE=str(self.root / "capacity.lock"),
            CRANPOSE_HOST_LOCK_TURNSTILE_FILE=str(self.root / "turnstile.lock"),
            CRANPOSE_HOST_LOCK_MAX_WAIT_SECS="2",
        )
        self.children = []
        self.addCleanup(self.stop_children)

    def stop_children(self):
        for child in self.children:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)
            child.stdout.close()

    def start(self, mode, code, *args):
        child = subprocess.Popen(
            [str(WRAPPER), mode, sys.executable, "-c", code, *map(str, args)],
            env=self.env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )
        self.children.append(child)
        return child

    def wait_file(self, path):
        deadline = time.monotonic() + 5
        while not path.exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertTrue(path.exists(), f"Command never created {path.name}")

    def holder(self, mode):
        ready = self.root / f"ready-{len(self.children)}"
        release = self.root / f"release-{len(self.children)}"
        child = self.start(
            mode,
            "from pathlib import Path; import sys,time; "
            "Path(sys.argv[1]).touch(); "
            "exec('while not Path(sys.argv[2]).exists(): time.sleep(0.02)')",
            ready,
            release,
        )
        self.wait_file(ready)
        return child, release

    def assert_success(self, child):
        output, _ = child.communicate(timeout=5)
        self.assertEqual(child.returncode, 0, output)

    def test_shared_commands_overlap(self):
        first, release = self.holder("--shared")
        second, other_release = self.holder("--shared")
        self.assertIsNone(first.poll())
        other_release.touch()
        self.assert_success(second)
        release.touch()
        self.assert_success(first)

    def test_exclusive_waits_for_shared_command(self):
        first, release = self.holder("--shared")
        entered = self.root / "exclusive-entered"
        second = self.start("--exclusive", "from pathlib import Path; import sys; Path(sys.argv[1]).touch()", entered)
        time.sleep(0.2)
        self.assertFalse(entered.exists(), "Exclusive command overlapped shared work")
        release.touch()
        self.assert_success(first)
        self.assert_success(second)
        self.assertTrue(entered.exists())

    def test_python_measurement_blocks_wrapper(self):
        entered = self.root / "entered"
        with open(self.env["CRANPOSE_HOST_LOCK_FILE"], "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            child = self.start("--shared", "from pathlib import Path; import sys; Path(sys.argv[1]).touch()", entered)
            time.sleep(0.2)
            self.assertFalse(entered.exists(), "Build overlapped a Python measurement")
            fcntl.flock(lock, fcntl.LOCK_UN)
            self.assert_success(child)
        self.assertTrue(entered.exists())

    def test_wrapper_blocks_python_measurement_until_command_exits(self):
        child, release = self.holder("--exclusive")
        with open(self.env["CRANPOSE_HOST_LOCK_FILE"], "a") as lock:
            with self.assertRaises(BlockingIOError):
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            release.touch()
            self.assert_success(child)
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_exit_status_and_child_signal_propagate(self):
        for code, expected in [
            ("raise SystemExit(37)", 37),
            ("import os,signal; os.kill(os.getpid(), signal.SIGTERM)", 143),
        ]:
            with self.subTest(code=code):
                child = self.start("--exclusive", code)
                output, _ = child.communicate(timeout=5)
                self.assertEqual(child.returncode, expected, output)
                with open(self.env["CRANPOSE_HOST_LOCK_FILE"], "a") as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_cancelled_process_group_releases_lock(self):
        child, _ = self.holder("--exclusive")
        os.killpg(child.pid, signal.SIGTERM)
        child.wait(timeout=5)
        with open(self.env["CRANPOSE_HOST_LOCK_FILE"], "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_nested_covered_commands_keep_outer_lock(self):
        for outer, inner in [
            ("--shared", "--shared"),
            ("--exclusive", "--shared"),
            ("--exclusive", "--exclusive"),
        ]:
            with self.subTest(outer=outer, inner=inner):
                child = self.start(
                    outer,
                    "import subprocess,sys; raise SystemExit(subprocess.call("
                    "[sys.argv[1],sys.argv[2],sys.executable,'-c','raise SystemExit(37)']))",
                    WRAPPER,
                    inner,
                )
                output, _ = child.communicate(timeout=5)
                self.assertEqual(child.returncode, 37, output)

    def test_missing_backends_refuse_to_run_command(self):
        (self.bin / "python3").unlink()
        entered = self.root / "unprotected"
        child = self.start("--shared", "from pathlib import Path; import sys; Path(sys.argv[1]).touch()", entered)
        output, _ = child.communicate(timeout=5)
        self.assertNotEqual(child.returncode, 0, output)
        self.assertFalse(entered.exists())

    def test_exclusive_timeout_does_not_run_command(self):
        entered = self.root / "timed-out"
        with open(self.env["CRANPOSE_HOST_LOCK_FILE"], "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_SH)
            child = self.start("--exclusive", "from pathlib import Path; import sys; Path(sys.argv[1]).touch()", entered)
            output, _ = child.communicate(timeout=5)
            self.assertNotEqual(child.returncode, 0, output)
        self.assertFalse(entered.exists())

    def faulty_backend(self, fault):
        backend = self.bin / "flock"
        backend.write_text(
            f"#!{sys.executable}\n"
            "import fcntl,os,sys\n"
            "fault=os.environ['LOCK_TEST_FAULT']\n"
            "if fault == 'all' or (fault != 'none' and '-s' in sys.argv):\n"
            "    raise SystemExit(1 if fault == 'wait' and '-n' in sys.argv else 73)\n"
            "fcntl.flock(int(sys.argv[-1]),fcntl.LOCK_SH if '-s' in sys.argv else fcntl.LOCK_EX)\n"
        )
        backend.chmod(0o755)
        self.env["LOCK_TEST_FAULT"] = fault

    def test_backend_errors_do_not_run_unprotected_commands(self):
        for fault in ("all", "shared", "wait"):
            with self.subTest(fault=fault):
                self.faulty_backend(fault)
                entered = self.root / fault
                child = self.start(
                    "--shared",
                    "from pathlib import Path; import sys; Path(sys.argv[1]).touch()",
                    entered,
                )
                output, _ = child.communicate(timeout=5)
                self.assertNotEqual(child.returncode, 0, output)
                self.assertFalse(entered.exists(), output)

    def test_robot_build_stops_when_backend_fails(self):
        fixture = self.root / "robot-fixture"
        (fixture / "scripts").mkdir(parents=True)
        runners = fixture / "apps/desktop-demo/robot-runners"
        runners.mkdir(parents=True)
        (runners / "robot_lock_fixture.rs").write_text("pub(crate) fn main() {}\n")
        repository = WRAPPER.parents[2]
        for relative in ("run_robot_test.sh", "scripts/dev_build_common.sh"):
            shutil.copy2(repository / relative, fixture / relative)
        self.env["PATH"] += os.pathsep + os.environ["PATH"]
        called = self.root / "cargo-called"
        cargo = self.bin / "cargo"
        cargo.write_text(f"#!/bin/sh\n: > '{called}'\nexit 79\n")
        cargo.chmod(0o755)
        self.env.update(CRANPOSE_ROBOT_PARALLEL="1", CRANPOSE_USE_LOCAL_TMPDIR="0", CARGO_BUILD_JOBS="1")
        for fault in ("none", "all", "shared"):
            with self.subTest(fault=fault):
                self.faulty_backend(fault)
                called.unlink(missing_ok=True)
                result = subprocess.run(
                    [str(fixture / "run_robot_test.sh"), "--build-only"],
                    cwd=fixture,
                    env=self.env,
                    capture_output=True,
                    text=True,
                    timeout=5,
                )
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(called.exists(), fault == "none", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
