"""The starts a round measures, and the frames in each second from a launch."""

# The two starts every round measures of each app: its first start after its
# install, with what it keeps between launches cleared as an install leaves
# it (its compiled shaders among them), and its second start after that.
STARTS = ('first', 'second')


def per_second(times, launched, run_s):
    """Frames presented in each whole second from `launched` to `launched +
    run_s`, from their presentation times, all on one clock in seconds."""
    counts = [0] * int(run_s)
    for time in times:
        second = int(time - launched)
        if 0 <= second < len(counts):
            counts[second] += 1
    return counts
