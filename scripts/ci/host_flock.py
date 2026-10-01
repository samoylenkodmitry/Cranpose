import argparse
import errno
import math
import sys
import time


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
    if not args.n and args.w is None:
        fcntl.flock(args.fd, operation)
        return 0
    deadline = time.monotonic() + (args.w or 0)
    while True:
        try:
            fcntl.flock(args.fd, operation | fcntl.LOCK_NB)
            return 0
        except OSError as error:
            if error.errno not in (errno.EACCES, errno.EAGAIN):
                raise
        remaining = deadline - time.monotonic()
        if args.n or remaining <= 0:
            return 1
        time.sleep(min(0.05, remaining))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ImportError) as error:
        print(f"host lock: {error}", file=sys.stderr)
        sys.exit(2)
