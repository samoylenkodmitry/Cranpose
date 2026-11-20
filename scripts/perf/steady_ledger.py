#!/usr/bin/env python3
"""The arithmetic of steady_ledger.sh: the rows of a run from its logs, the
verdict against the budget file, the budget lines of repeated runs, and the
difference of two runs.

A rows file holds one value a line, `ROW METRIC VALUE`; the budget file one
budget a line, `ROW METRIC BUDGET SPREAD`.

  valgrind --trace-malloc=yes ... 2>&1 | steady_ledger.py trace > LOG
  steady_ledger.py rows RUN_DIR FRAMES START_ROW GROUPS TREE > ROWS
  steady_ledger.py compare ROWS BUDGET GROUPS PHASED
  steady_ledger.py spread ROWS...
  steady_ledger.py diff BUDGET GROUPS BASE_ROWS HEAD_ROWS
"""

import json
import os
import re
import sys
from collections import Counter, defaultdict, deque, namedtuple

# The values of a row, in the order of the printed tables.
METRICS = ("malloc", "realloc", "live", "copied", "realloc_bytes")
LABELS = ("malloc", "realloc", "live", "copied B", "realloc B")
GROUP = "group:"
BYTES = ("copied", "realloc_bytes")


def tolerance(metric):
    # The budget file rounds calls to 0.01 and bytes to 1.
    return 0.5 if metric in BYTES else 0.005


# The share of a budget that copied bytes may move by beyond their spread:
# inlining that an unrelated change shifts moves them a little.
BYTES_SHARE = 0.02


def slack(metric, budget):
    """How far a value may pass its budget plus its spread."""
    return max(tolerance(metric), BYTES_SHARE * abs(budget)) if metric in BYTES else tolerance(metric)

# valgrind's `--trace-malloc=yes` prints a call, then ` = ADDRESS` once it
# returns; another thread's calls, or the bench's own lines, can come
# between the two, on the same line or on the next.
TRACE_TOKEN = re.compile(
    r"\b(malloc|calloc|realloc|free|posix_memalign|memalign|aligned_alloc)\(([^()]*)\)"
    r"|= (0x[0-9A-Fa-f]+)|steady_frames: phase (\w+)")
PHASE = re.compile(r"steady_frames: phase (\w+)")


def bench_counts(path):
    """The key=value pairs of the bench's `steady_frames` line."""
    with open(path) as handle:
        for line in handle:
            if line.startswith("steady_frames "):
                return {key: int(value) for key, value in
                        (pair.split("=", 1) for pair in line.split()[1:])}
    raise SystemExit(f"steady_ledger: no counts in {path}")


# The standard library's `realloc` of a block aligned past what the C
# library's `realloc` keeps: a new block and a memcpy.
STD_REALLOC = re.compile(r"\brealloc_fallback\b|\b__rust_realloc\b")


# The example's functions whose stacks make up its phases.
START = "steady_frames::start"
STEADY = "steady_frames::steady_frames"
# The outermost frame of a whole stack: a thread's entry. valgrind keeps
# the innermost 500 frames, so a deep layout's stack can end before it.
ENTRY = re.compile(r"^0x[0-9A-Fa-f]+: (main|start_thread|clone3?|\(below main\)) \(")


def points(path, window):
    """The program points of the DHAT profile at PATH as lists of frames,
    with their counts and whether their stack is whole; with WINDOW, the
    whole stacks that hold that function and every cut stack."""
    data = json.load(open(path))
    table = data["ftbl"]
    marks = {index for index, entry in enumerate(table)
             if (frame := FRAME.match(entry)) and frame.group(1) == window}
    for point in data["pps"]:
        indices = point["fs"]
        whole = bool(ENTRY.match(table[indices[-1]]))
        if window is None or not whole or marks.intersection(indices):
            yield [table[index] for index in indices], point, whole


# Bytes that DHAT counted: outside `realloc` and in the standard library's
# `realloc`, in whole stacks of a window and in cut stacks.
Copies = namedtuple("Copies", "outside inside cut_outside cut_inside")


def copies(path, window=None):
    """The bytes that valgrind's DHAT in copy mode counted in the run of the
    profile at PATH, in WINDOW."""
    sums = Counter()
    for frames, point, whole in points(path, window):
        kind = "inside" if any(STD_REALLOC.search(frame) for frame in frames) else "outside"
        sums[kind if whole else f"cut_{kind}"] += point["tb"]
    return Copies(*(sums[field] for field in Copies._fields))


def window_copies(full, empty, window):
    """The bytes copied in WINDOW, outside `realloc` and in it: whole stacks
    of the run at FULL place theirs exactly; for cut stacks, those of FULL
    minus those of EMPTY, the same run with an empty window."""
    run, base = copies(full, window), copies(empty, window)
    return (run.outside + run.cut_outside - base.cut_outside,
            run.inside + run.cut_inside - base.cut_inside)


TRACED = ("fresh", "realloc", "free", "realloc_bytes")


def trace_filter(source, sink):
    """Passes valgrind's log through and sums the allocator calls that
    `--trace-malloc=yes` prints, by phase: fresh blocks, `realloc` calls,
    frees, and the bytes each `realloc` keeps (the smaller of the old and the
    new size), which it copies when it moves the block. Phases are the
    bench's `steady_frames: phase` lines; a run without them is one phase,
    `all`. Appends one `steady_ledger trace` line a phase, so that the log
    stays small."""
    sizes = {}
    counts = defaultdict(Counter)
    bucket = counts["all"]
    # The sizes of the calls that wait for their address, oldest first.
    pending = deque()
    for line in source:
        if not line.startswith("--"):
            sink.write(line)
            marker = PHASE.search(line)
            if marker:
                bucket = counts[marker.group(1)]
            continue
        tokens = list(TRACE_TOKEN.finditer(line))
        if not tokens:
            sink.write(line)
            continue
        for token in tokens:
            name, args, address, phase = token.groups()
            if phase:
                bucket = counts[phase]
            elif address:
                if pending:
                    sizes[address] = pending.popleft()
            elif name == "free":
                if args != "0x0":
                    bucket["free"] += 1
                    sizes.pop(args, None)
            elif name == "realloc":
                old, new = args.split(",")
                bucket["realloc"] += 1
                bucket["realloc_bytes"] += min(sizes.pop(old, 0), int(new))
                pending.append(int(new))
            else:
                bucket["fresh"] += 1
                if name == "malloc":
                    pending.append(int(args))
                elif name == "calloc":
                    count, size = args.split(",")
                    pending.append(int(count) * int(size))
                else:
                    pending.append(int(args.rsplit(" ", 1)[1]))
    for phase, bucket in counts.items():
        sink.write(f"steady_ledger trace {phase} "
                   + " ".join(f"{key}={bucket[key]}" for key in TRACED) + "\n")
    return 0


def trace(path):
    """The sums that trace_filter appended to the log at PATH, by phase."""
    counts = defaultdict(Counter)
    with open(path, errors="replace") as handle:
        for line in handle:
            if line.startswith("steady_ledger trace "):
                _, _, phase, *pairs = line.split()
                counts[phase] = Counter({key: int(value) for key, value in
                                         (pair.split("=", 1) for pair in pairs)})
    if not counts:
        raise SystemExit(f"steady_ledger: no allocator trace in {path}")
    return counts


def load_groups(path):
    """The groups of the start's allocator calls: name, target and the
    regexes of their sites, in the file's order."""
    groups = {}
    with open(path) as handle:
        for line in handle:
            if not line.strip() or line.startswith("#"):
                continue
            name, target, pattern = line.split(None, 2)
            entry = groups.setdefault(name, [float(target), []])
            entry[1].append(re.compile(pattern.strip()))
    return groups


# A DHAT frame: `0xADDRESS: FUNCTION (FILE:LINE)`; the function's name can
# hold parentheses itself, its location none.
FRAME = re.compile(r"^0x[0-9A-Fa-f]+: (.*) \(([^()]*)\)$")


# The example itself and its window: its counting allocator lies on every
# stack.
HARNESS = ("benchmarks/compose-vs-cranpose/cranpose-app/examples/",
           "benchmarks/compose-vs-cranpose/cranpose-app/benches/")


def site(frames, tree):
    """`FUNCTION @ FILE:LINE` of the first frame whose source lies in the
    tree outside the bench, FILE relative to the tree; valgrind's
    `--fullpath-after=` gives full paths."""
    for frame in frames:
        match = FRAME.match(frame)
        if match and match.group(2).startswith(tree):
            location = match.group(2)[len(tree):]
            if not location.startswith(HARNESS):
                return f"{match.group(1)} @ {location}"
    return None


def group_calls(path, groups, tree):
    """Allocator calls of the start in the DHAT heap profile at PATH by
    group; calls whose site no group names count as `other`."""
    calls = Counter()
    placed = 0
    for frames, point, whole in points(path, START):
        if not whole:
            raise SystemExit(f"steady_ledger: {path} holds cut stacks, which no group can place")
        where = site(frames, tree)
        placed += where is not None
        name = next((name for name, (_, patterns) in groups.items()
                     if any(pattern.search(where or "") for pattern in patterns)), "other")
        calls[name] += point["tbk"]
    if not placed:
        raise SystemExit(f"steady_ledger: no allocation of {path} has a frame in {tree}: "
                         "the build's source paths differ from the tree's")
    return calls


# The instructions that an A/B counts under callgrind; the nightly does not.
INSTRUCTIONS = "instructions"
TOTALS = re.compile(r"^(?:totals|summary): (\d+)")


def instructions(row, path, count):
    """ROW's instructions a frame or a node from the callgrind profile at
    PATH, when the run made one."""
    if not os.path.exists(path):
        return []
    with open(path) as handle:
        for line in handle:
            total = TOTALS.match(line)
            if total:
                return [(row, INSTRUCTIONS, int(total.group(1)) / count)]
    raise SystemExit(f"steady_ledger: no totals in {path}")


def rows(run, frames, start_row, groups_path, tree):
    """The rows of a run: every steady row, the start row and its groups, and
    the dashboard. TREE is the source tree of the build."""
    tree = tree.rstrip("/") + "/"
    out = []
    order = open(f"{run}/order").read().split()
    for row in order:
        base = f"{run}/{row}"
        if row == "dashboard":
            zero, full = trace(f"{base}.fb-0")["all"], trace(f"{base}.fb-{frames}")["all"]
            run, base_run = copies(f"{base}.fb-{frames}.json"), copies(f"{base}.fb-0.json")
            outside = run.outside + run.cut_outside - base_run.outside - base_run.cut_outside
            inside = run.inside + run.cut_inside - base_run.inside - base_run.cut_inside
            values = {
                "malloc": full["fresh"] - zero["fresh"],
                "realloc": full["realloc"] - zero["realloc"],
                "live": full["fresh"] - full["free"] - zero["fresh"] + zero["free"],
                "copied": outside,
                "realloc_bytes": full["realloc_bytes"] - zero["realloc_bytes"] + inside,
            }
            out.extend((row, metric, values[metric] / frames) for metric in METRICS)
            out.extend(instructions(row, f"{base}.fb-instr.out", frames))
            out.extend(instructions(f"{row}-rgba", f"{base}.fb-instr-rgba.out", frames))
            continue
        counts = bench_counts(f"{base}.count")
        traced = trace(f"{base}.copy")
        steady = counts["frames"]
        outside, inside = window_copies(f"{base}.copy.json", f"{base}.empty.json", STEADY)
        values = {
            "malloc": counts["malloc"],
            "realloc": counts["realloc"],
            "live": counts["malloc"] - counts["free"],
            "copied": outside,
            "realloc_bytes": traced["steady"]["realloc_bytes"] + inside,
        }
        out.extend((row, metric, values[metric] / steady) for metric in METRICS)
        out.extend(instructions(row, f"{base}.instr.out", steady))
        if row == start_row:
            nodes = counts["nodes"]
            started = copies(f"{base}.copy.json", START)
            if started.cut_outside or started.cut_inside:
                raise SystemExit(f"steady_ledger: {base}.copy.json holds cut stacks, "
                                 "which the start row cannot place")
            values = {
                "malloc": counts["start_malloc"],
                "realloc": counts["start_realloc"],
                "live": counts["start_malloc"] - counts["start_free"],
                "copied": started.outside,
                "realloc_bytes": traced["start"]["realloc_bytes"] + started.inside,
            }
            out.extend((f"{row}-start", metric, values[metric] / nodes) for metric in METRICS)
            out.extend(instructions(f"{row}-start", f"{base}.instr-start.out", nodes))
            groups = load_groups(groups_path)
            calls = group_calls(f"{base}.heap.json", groups, tree)
            for name in list(groups) + ["other"]:
                out.append((f"{row}-start", GROUP + name, calls[name] / nodes))
    return out


def read_rows(path):
    table = {}
    for line in open(path):
        row, metric, value = line.split()
        table.setdefault(row, {})[metric] = float(value)
    return table


def read_budget(path):
    budget = {}
    for line in open(path):
        if not line.strip() or line.startswith("#"):
            continue
        row, metric, value, spread = line.split()
        budget.setdefault(row, {})[metric] = (float(value), float(spread))
    return budget


def cell(metric, value):
    return f"{value:.0f}" if metric in BYTES else f"{value:.2f}"


def header():
    return f"{'row':<16}" + "".join(f" {label:>10}" for label in LABELS)


def group_line(row, values, groups, extra=lambda name: ""):
    parts = []
    for name in list(groups) + ["other"]:
        target = f" [{groups[name][0]:g}]" if name in groups else ""
        parts.append(f"{name} {values.get(GROUP + name, 0):.2f}{target}{extra(name)}")
    return f"{row} by group, calls a node [target]: " + "  ".join(parts)


def compare(rows_path, budget_path, groups_path, phased):
    """Prints every row with its verdict; exits 1 when a value passes its
    budget plus its spread and its slack, or a row and its budget do not
    match up. The rows named in PHASED, whose scene changes in the window,
    print their live count and leave it out of the verdict."""
    table, budget, groups = read_rows(rows_path), read_budget(budget_path), load_groups(groups_path)
    phased = set(phased.split())
    failed = False
    print(header() + "   verdict")
    for row, values in table.items():
        rose, fell = [], []
        for metric, value in values.items():
            if metric == "live" and row in phased:
                continue
            if row not in budget or metric not in budget[row]:
                rose.append(f"{metric} (no budget)")
                continue
            limit, spread = budget[row][metric]
            if value > limit + spread + slack(metric, limit):
                rose.append(metric)
            elif value < limit - spread - slack(metric, limit):
                fell.append(metric)
        failed |= bool(rose)
        verdict = ("ROSE " + " ".join(rose) if rose
                   else "fell " + " ".join(fell) if fell else "ok")
        print(f"{row:<16}" + "".join(f" {cell(m, values.get(m, 0)):>10}" for m in METRICS)
              + f"   {verdict}")
        if any(metric.startswith(GROUP) for metric in values):
            print("  " + group_line(row, values, groups))
    for row, metrics in budget.items():
        missing = [metric for metric in metrics if metric not in table.get(row, {})]
        if missing:
            print(f"{row:<16} missing from the run: {' '.join(missing)}")
            failed = True
    return 1 if failed else 0


def spread(paths):
    """Budget lines: the highest value of each row and metric over the runs,
    and the spread, the highest minus the lowest."""
    seen = {}
    for path in paths:
        for line in open(path):
            row, metric, value = line.split()
            seen.setdefault((row, metric), []).append(float(value))
    for (row, metric), values in seen.items():
        high, low = max(values), min(values)
        if metric in BYTES:
            print(f"{row:<16} {metric:<20} {high:>12.0f} {high - low:>10.0f}")
        else:
            print(f"{row:<16} {metric:<20} {high:>12.2f} {high - low:>10.2f}")
    return 0


def diff(budget_path, groups_path, base_path, head_path):
    """The head's values and head minus base a row; `*` marks a change past
    the spread in the budget file."""
    budget, groups = read_budget(budget_path), load_groups(groups_path)
    base, head = read_rows(base_path), read_rows(head_path)

    def change(row, metric):
        delta = head[row].get(metric, 0) - base.get(row, {}).get(metric, 0)
        spread = budget.get(row, {}).get(metric, (0, 0))[1]
        return delta, abs(delta) > spread + tolerance(metric)

    print("head values, then head minus base; * marks a change past its spread")
    print(f"{'row':<16}" + "".join(f" {label:>10} {'diff':>8}" for label in LABELS))
    for row, values in head.items():
        # A row of instructions alone prints in the table below.
        if not any(metric in values for metric in METRICS):
            continue
        line, marked = f"{row:<16}", False
        for metric in METRICS:
            delta, past = change(row, metric)
            marked |= past
            sign = f"{delta:+.0f}" if metric in BYTES else f"{delta:+.2f}"
            line += f" {cell(metric, values.get(metric, 0)):>10} {sign:>8}"
        print(line + (" *" if marked else ""))
        if any(metric.startswith(GROUP) for metric in values):
            def extra(name):
                delta, past = change(row, GROUP + name)
                return f" ({delta:+.2f}{'*' if past else ''})"
            print("  " + group_line(row, values, groups, extra))
    measured = [row for row, values in head.items() if INSTRUCTIONS in values]
    if measured:
        print("instructions (callgrind), a frame; the start a node")
        print(f"{'row':<16} {'base':>14} {'head':>14} {'diff':>9}")
        for row in measured:
            after = head[row][INSTRUCTIONS]
            before = base.get(row, {}).get(INSTRUCTIONS, 0)
            share = f"{100 * (after - before) / before:+.2f}%" if before else "new"
            print(f"{row:<16} {before:>14,.0f} {after:>14,.0f} {share:>9}")
    return 0


def main(argv):
    command = argv[1] if len(argv) > 1 else ""
    if command == "rows":
        for row, metric, value in rows(argv[2], int(argv[3]), argv[4], argv[5], argv[6]):
            print(f"{row} {metric} {value:.6f}")
        return 0
    if command == "trace":
        return trace_filter(sys.stdin, sys.stdout)
    if command == "compare":
        return compare(argv[2], argv[3], argv[4], argv[5])
    if command == "spread":
        return spread(argv[2:])
    if command == "diff":
        return diff(argv[2], argv[3], argv[4], argv[5])
    raise SystemExit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
