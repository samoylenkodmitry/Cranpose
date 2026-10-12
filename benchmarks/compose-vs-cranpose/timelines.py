"""The starts a round measures, the rule that ends a slow run, and the frames
in each second from a launch."""

# The two starts every round measures of each app: its first start after its
# install, with what it keeps between launches cleared as an install leaves
# it (its compiled shaders among them), and its second start after that.
STARTS = ('first', 'second')

# A run whose app presented fewer than SLOW_FPS frames in each of the last
# SLOW_SECONDS whole seconds since its launch ends there: the rest would only
# repeat that it is slow. Seconds in a row, not an average, so a slow start
# that the app then outruns does not end it.
SLOW_FPS = 30
SLOW_SECONDS = 5


def slow_rule():
    """The rule for a slow run, as every run's protocol records it."""
    return {'stop_below_fps': SLOW_FPS, 'stop_seconds_in_a_row': SLOW_SECONDS}


def per_second(times, launched, run_s):
    """Frames presented in each whole second from `launched` to `launched +
    run_s`, from their presentation times, all on one clock in seconds."""
    counts = [0] * int(run_s)
    for time in times:
        second = int(time - launched)
        if 0 <= second < len(counts):
            counts[second] += 1
    return counts
