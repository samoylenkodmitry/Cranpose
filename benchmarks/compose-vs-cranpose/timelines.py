"""Frames in each second from an app's launch."""


def per_second(times, launched, run_s):
    """Frames presented in each whole second from `launched` to `launched +
    run_s`, from their presentation times, all on one clock in seconds."""
    counts = [0] * int(run_s)
    for time in times:
        second = int(time - launched)
        if 0 <= second < len(counts):
            counts[second] += 1
    return counts
