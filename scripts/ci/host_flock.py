import argparse
import contextlib
import errno
import math
import os
import sys
import time

DEFAULT_LOCK_FILE = "/tmp/cranpose-host-capacity.lock"
DEFAULT_TURNSTILE_FILE = "/tmp/cranpose-host-capacity.turnstile.lock"
DEFAULT_MAX_WAIT_SECS = 2700.0


def acquire_fd(fd, operation, *, nonblocking=False, wait_seconds=None):
    import fcntl

    if wait_seconds is not None and (not math.isfinite(wait_seconds) or wait_seconds < 0):
        raise ValueError("wait duration must be finite and nonnegative")
    if not nonblocking and wait_seconds is None:
        fcntl.flock(fd, operation)
        return True
    deadline = time.monotonic() + (wait_seconds or 0.0)
    while True:
        try:
            fcntl.flock(fd, operation | fcntl.LOCK_NB)
            return True
        except OSError as error:
            if error.errno not in (errno.EACCES, errno.EAGAIN):
                raise
        remaining = deadline - time.monotonic()
        if nonblocking or remaining <= 0:
            return False
        time.sleep(min(0.05, remaining))


@contextlib.contextmanager
def exclusive_lock(max_wait_seconds=None):
    import fcntl

    held = os.environ.get("CRANPOSE_HOST_LOCK_HELD")
    if held == "exclusive":
        yield
        return
    if held == "shared":
        raise RuntimeError("cannot take exclusive host lock inside a shared host lock")

    lock_path = os.environ.get("CRANPOSE_HOST_LOCK_FILE") or DEFAULT_LOCK_FILE
    turnstile_path = (
        os.environ.get("CRANPOSE_HOST_LOCK_TURNSTILE_FILE") or DEFAULT_TURNSTILE_FILE
    )
    if max_wait_seconds is None:
        max_wait_seconds = float(
            os.environ.get("CRANPOSE_HOST_LOCK_MAX_WAIT_SECS", DEFAULT_MAX_WAIT_SECS)
        )
    deadline = time.monotonic() + max_wait_seconds

    turnstile = open(turnstile_path, "a")
    lock = None
    try:
        remaining = max(0.0, deadline - time.monotonic())
        if not acquire_fd(turnstile.fileno(), fcntl.LOCK_EX, wait_seconds=remaining):
            raise TimeoutError(
                f"host lock: gave up waiting for turnstile {turnstile_path} after {max_wait_seconds:.0f}s"
            )
        lock = open(lock_path, "a")
        remaining = max(0.0, deadline - time.monotonic())
        if not acquire_fd(lock.fileno(), fcntl.LOCK_EX, wait_seconds=remaining):
            raise TimeoutError(
                f"host lock: gave up waiting for exclusive lock {lock_path} after {max_wait_seconds:.0f}s"
            )
    except BaseException:
        if lock is not None:
            lock.close()
        turnstile.close()
        raise

    # Release the turnstile once the exclusive lock is held so waiting readers
    # queue on the capacity lock and pass when this leg releases it.
    turnstile.close()
    os.environ["CRANPOSE_HOST_LOCK_HELD"] = "exclusive"
    try:
        yield
    finally:
        if held is None:
            os.environ.pop("CRANPOSE_HOST_LOCK_HELD", None)
        else:
            os.environ["CRANPOSE_HOST_LOCK_HELD"] = held
        lock.close()


def main():
    import fcntl

    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("-s", action="store_true")
    mode.add_argument("-x", action="store_true")
    parser.add_argument("-n", action="store_true")
    parser.add_argument("-w", type=float)
    parser.add_argument("fd", type=int)
    args = parser.parse_args()
    if args.w is not None and (not math.isfinite(args.w) or args.w < 0):
        parser.error("wait duration must be finite and nonnegative")
    operation = fcntl.LOCK_SH if args.s else fcntl.LOCK_EX
    return 0 if acquire_fd(args.fd, operation, nonblocking=args.n, wait_seconds=args.w) else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ImportError) as error:
        print(f"host lock: {error}", file=sys.stderr)
        sys.exit(2)
