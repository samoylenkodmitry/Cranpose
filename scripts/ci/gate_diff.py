#!/usr/bin/env python3
"""Which line ranges a change touches, for the diff-scoped quality gates.

`complexity_gate.py` and `duplication_gate.py` both need the same answer to
the same question: which lines, in which files, did this change add or
modify? Neither gate is allowed to fail on code the diff never touched, so
both derive their scope from here rather than from a repo-wide scan.

"The diff" is `git diff --unified=0 --no-prefix <merge-base>`, with no second
ref: that compares the merge base against the working tree, which also covers
staged and unstaged edits a developer has not committed yet. In CI the working
tree is exactly the checked-out commit, so the same command means the same
thing in both places.

The merge base is computed against the ref given (default `origin/main`), not
hardcoded to a branch name or a commit, so this works for any PR and for a
plain feature branch alike. A shallow checkout (`actions/checkout`'s default)
leaves `base` and `HEAD` as two problems, not one: `ensure_ref` makes `base`
resolvable by fetching it, and `merge_base` separately makes it *reachable*
from `HEAD` by deepening history when the two sides turn out to be
disconnected single-commit graphs with no merge base between them.
"""

from __future__ import annotations

import os
import re
import subprocess
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

_HUNK_HEADER = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def ensure_ref(base: str) -> None:
    """Make `base` resolvable and current.

    A persistent self-hosted runner's checkout is not fresh by default: a
    prior job can leave `origin/main` resolvable but pointing at a stale
    commit, which computes a stale merge base and silently mis-scopes the
    diff rather than failing loudly. So this always re-fetches `base` when it
    looks like `origin/<branch>`, rather than only fetching when the ref is
    entirely missing (the shallow-checkout case). A fetch failure is only
    fatal if `base` does not already resolve to *something* locally -- a
    developer running this offline with a same-day `origin/main` should not
    be blocked by the network.
    """
    if base.startswith("origin/"):
        branch = base.removeprefix("origin/")
        subprocess.run(["git", "fetch", "--quiet", "origin", branch], cwd=ROOT)
    resolved = subprocess.run(
        ["git", "rev-parse", "--verify", "-q", base],
        cwd=ROOT,
        capture_output=True,
    )
    if resolved.returncode != 0:
        raise SystemExit(
            f"gate_diff: cannot resolve base ref {base!r}: it fetched nothing "
            "and no local ref by that name exists"
        )


def is_shallow_repository() -> bool:
    """Whether the checkout's history is truncated at some boundary commit."""
    result = subprocess.run(
        ["git", "rev-parse", "--is-shallow-repository"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip() == "true"


def merge_base(base: str) -> str:
    """The merge base of `base` and HEAD, deepening history first if needed.

    `actions/checkout`'s default depth-1 clone gives `HEAD` a history of
    exactly one commit with no recorded parent; `ensure_ref`'s own fetch of
    `base` is likewise shallow. The two land as disconnected single-commit
    graphs, so `git merge-base` exits 1 even though `base` resolves fine --
    not because the branches lack a common ancestor, but because neither
    side's local history reaches back far enough to see it. `git fetch
    --unshallow` is the fix, tried only after a first attempt fails and only
    when the repository is actually shallow: a non-shallow repository
    failing the same merge-base call really does share no history with
    `base`, and this refuses to paper over that by guessing a diff scope --
    the exact silent mis-scoping the diff-scoped gates exist to prevent.
    """
    ensure_ref(base)
    first_attempt = subprocess.run(
        ["git", "merge-base", base, "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if first_attempt.returncode == 0:
        return first_attempt.stdout.strip()
    if not is_shallow_repository():
        raise SystemExit(
            f"gate_diff: no merge base between {base!r} and HEAD, and the "
            "checkout already has full history -- these two branches share "
            "no common ancestor, so the diff cannot be scoped"
        )
    deepened = subprocess.run(["git", "fetch", "--quiet", "--unshallow", "origin"], cwd=ROOT)
    if deepened.returncode != 0:
        raise SystemExit(
            "gate_diff: the checkout is shallow and `git fetch --unshallow "
            "origin` failed -- cannot deepen history enough to find a "
            f"merge base with {base!r}"
        )
    second_attempt = subprocess.run(
        ["git", "merge-base", base, "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if second_attempt.returncode != 0:
        raise SystemExit(
            f"gate_diff: no merge base between {base!r} and HEAD even after "
            "`git fetch --unshallow` -- cannot scope the diff safely"
        )
    return second_attempt.stdout.strip()


def parse_hunk_spans(
    diff_text: str,
) -> dict[str, list[tuple[tuple[int, int] | None, tuple[int, int] | None]]]:
    """Parse `git diff --unified=0 --no-prefix` output into per-hunk spans.

    Each hunk becomes one `(old_span, new_span)` pair, keyed by the file's
    *new*-side path. `old_span` is the hunk's inclusive `(start, end)` range
    in the pre-change file's line numbering, `new_span` the same in the
    post-change file's; either is `None` when that side contributed zero
    lines (`old_span` for a pure addition, `new_span` for a pure deletion).

    `semantic_ranges` needs both sides: deciding whether a hunk changed
    anything a reader would call code means reading what was actually there
    before, not just guessing from the new side alone. A deleted file
    (`+++ /dev/null`) is skipped -- nothing in it can be checked against,
    because nothing in it still exists.
    """
    hunks: dict[str, list[tuple[tuple[int, int] | None, tuple[int, int] | None]]] = {}
    current_file: str | None = None
    for line in diff_text.splitlines():
        if line.startswith("+++ "):
            path = line[len("+++ ") :]
            current_file = None if path == "/dev/null" else path
            continue
        if not line.startswith("@@ "):
            continue
        if current_file is None:
            continue
        match = _HUNK_HEADER.match(line)
        if not match:
            continue
        old_start = int(match.group(1))
        old_count = int(match.group(2)) if match.group(2) is not None else 1
        new_start = int(match.group(3))
        new_count = int(match.group(4)) if match.group(4) is not None else 1
        old_span = None if old_count == 0 else (old_start, old_start + old_count - 1)
        new_span = None if new_count == 0 else (new_start, new_start + new_count - 1)
        hunks.setdefault(current_file, []).append((old_span, new_span))
    return hunks


_RAW_STRING_START = re.compile(r"(?:b?r)(?P<hashes>#{0,255})\"")
_CHAR_LITERAL = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^'\\\n])'")
_IDENT_CHAR = re.compile(r"[A-Za-z0-9_]")


def code_tokens_by_line(text: str) -> list[list[str]]:
    r"""Per physical line of `text` (0-indexed), the code tokens that start
    on it -- comments and whitespace contribute nothing.

    A single forward scan over the *whole* file, not just a hunk in
    isolation: a `git diff --unified=0` hunk carries no surrounding context,
    so a block comment that opened on an earlier, unchanged line is
    invisible to anything that only looks at the hunk's own text. Scanning
    from byte zero of the real file tracks block-comment depth (Rust nests
    `/* /* */ */`) and string state correctly regardless of where a hunk's
    boundaries happen to fall.

    Deliberately coarse-grained, on purpose: a maximal run of identifier
    characters (`[A-Za-z0-9_]`) is one token, because a real lexer boundary
    is the one thing whitespace or its absence actually decides (`let x` is
    two tokens, `letx` is one, and that distinction must survive). Every
    other significant byte -- each individual operator or punctuation
    character, and each string / raw string / char literal as a single
    whole -- is its own token. This never conflates two different inputs:
    splitting a multi-character operator like `->` into two single-
    character tokens only ever adds detail that could distinguish two
    snippets, it never merges anything that was distinct before. Two
    renderings of the same code -- different indentation, a comment removed,
    a multi-line block collapsed onto one line -- always produce equal token
    lists; nothing that changes what the code actually does can produce an
    equal one, because it must add, remove, or replace at least one token.

    Deliberately conservative in one direction only, same as the scan this
    replaced: every ambiguous case below resolves toward "this is code",
    never toward "this is a comment". Classifying a real code byte as
    comment would silently shrink the gate's scope; classifying a comment
    byte as code only costs `semantic_ranges` the precision of noticing two
    sides are equal when they truly are not (it still never lets something
    through the gate). A bare quote is the sharp edge: `'a` is a lifetime,
    not the start of an unterminated char literal, so it is only treated as
    opening a char literal when a closing quote is visible within a few
    characters, matching a real `'c'` or a `'\n'` / `'\x41'` / `'\u{1F600}'`
    escape -- otherwise the quote is emitted as its own one-character token
    and scanning continues normally, so a lifetime can never swallow the
    rest of the file.
    """
    lines = text.split("\n")
    tokens: list[list[str]] = [[] for _ in lines]
    line_idx = 0
    i = 0
    n = len(text)
    block_depth = 0

    while i < n:
        ch = text[i]

        if ch == "\n":
            line_idx += 1
            i += 1
            continue

        if block_depth > 0:
            if text.startswith("/*", i):
                block_depth += 1
                i += 2
            elif text.startswith("*/", i):
                block_depth -= 1
                i += 2
            else:
                i += 1
            continue

        if ch in " \t\r":
            i += 1
            continue

        if text.startswith("//", i):
            end = text.find("\n", i)
            i = n if end == -1 else end
            continue

        if text.startswith("/*", i):
            block_depth = 1
            i += 2
            continue

        prev_char = text[i - 1] if i > 0 else ""
        if not _IDENT_CHAR.match(prev_char):
            raw_match = _RAW_STRING_START.match(text, i)
            if raw_match:
                end_marker = '"' + raw_match.group("hashes")
                end = text.find(end_marker, raw_match.end())
                if end == -1:
                    tokens[line_idx].append(text[i:])
                    i = n
                    continue
                span_end = end + len(end_marker)
                tokens[line_idx].append(text[i:span_end])
                line_idx += text.count("\n", i, span_end)
                i = span_end
                continue

        if ch == '"':
            start = i
            i += 1
            while i < n:
                c = text[i]
                if c == "\\" and i + 1 < n:
                    i += 2
                    continue
                i += 1
                if c == '"':
                    break
            span = text[start:i]
            tokens[line_idx].append(span)
            line_idx += span.count("\n")
            continue

        if ch == "'":
            match = _CHAR_LITERAL.match(text, i)
            if match:
                tokens[line_idx].append(text[i : match.end()])
                i = match.end()
            else:
                tokens[line_idx].append(ch)
                i += 1
            continue

        if _IDENT_CHAR.match(ch):
            start = i
            while i < n and _IDENT_CHAR.match(text[i]):
                i += 1
            tokens[line_idx].append(text[start:i])
            continue

        tokens[line_idx].append(ch)
        i += 1

    return tokens


def _tokens_in_span(tokens_by_line: list[list[str]], span: tuple[int, int] | None) -> list[str]:
    """The tokens `code_tokens_by_line` attributed to lines `span[0]..=span[1]`."""
    if span is None:
        return []
    start, end = span
    result: list[str] = []
    for i in range(start - 1, min(end, len(tokens_by_line))):
        result.extend(tokens_by_line[i])
    return result


_CLOSING_DELIMITERS = frozenset({")", "]", "}"})


def _drop_trailing_commas(tokens: list[str]) -> list[str]:
    """`tokens`, minus every `,` immediately followed by a closing delimiter.

    A trailing comma right before `)`, `]`, or `}` is never semantically
    significant in Rust: a tuple, array, function call, generic argument
    list, struct literal, or the last arm of a `match` parses identically
    with or without one. Whether rustfmt emits it depends only on its
    multi-line-vs-single-line layout choice for the surrounding
    expression -- which can flip purely because a comment that used to
    keep the expression multi-line is gone, with nothing about the code
    itself changed. That is exactly the "same code, different whitespace"
    class `semantic_ranges` exists to see through, so this one narrow,
    always-valid comma is dropped before old and new token lists are
    compared, on both sides alike.

    Deliberately this narrow: this is a fact about Rust's grammar, not a
    general "whitespace doesn't matter" rule, and it changes nothing about
    how any other token is compared.
    """
    return [
        tok
        for idx, tok in enumerate(tokens)
        if not (tok == "," and idx + 1 < len(tokens) and tokens[idx + 1] in _CLOSING_DELIMITERS)
    ]


def semantic_ranges(
    hunk_spans: dict[str, list[tuple[tuple[int, int] | None, tuple[int, int] | None]]],
    read_old: Callable[[str], str],
    read_new: Callable[[str], str],
) -> dict[str, list[tuple[int, int]]]:
    """`hunk_spans`, narrowed to the hunks that changed actual code.

    A hunk keeps its `new_span` the moment the tokens it changed differ from
    what was there before: the existing "touching a bad function makes you
    fix it" behavior for a genuine logic edit is unchanged, deliberately.
    What is new is *how* "differ" gets decided -- both sides of the hunk are
    read back from their real files (the pre-change blob via `read_old`, the
    post-change file via `read_new`, both keyed by the new-side path) and
    tokenized with `code_tokens_by_line`, then passed through
    `_drop_trailing_commas` (see there for why that one further step is
    Rust-grammar-safe rather than a general whitespace rule), so a hunk is
    dropped exactly when its old and new token lists are equal: a comment
    deleted, a comment added with nothing else, a multi-line block
    reformatted or collapsed onto one line with no token added or removed.
    This subsumes the narrower "hunk is entirely comment" rule this
    replaced without a separate code path for it: a comment-only hunk's old
    and new token lists were already both empty, so it always falls out of
    the same single comparison.

    A pure-deletion hunk (`new_span` is `None`) is dropped unconditionally,
    regardless of what its old side tokenizes to: there is no line left on
    the new side to hold accountable.
    """
    ranges: dict[str, list[tuple[int, int]]] = {}
    old_cache: dict[str, list[list[str]]] = {}
    new_cache: dict[str, list[list[str]]] = {}
    for file, spans in hunk_spans.items():
        if file not in new_cache:
            new_cache[file] = code_tokens_by_line(read_new(file))
        if file not in old_cache:
            old_cache[file] = code_tokens_by_line(read_old(file))
        old_tokens_by_line = old_cache[file]
        new_tokens_by_line = new_cache[file]
        kept: list[tuple[int, int]] = []
        for old_span, new_span in spans:
            if new_span is None:
                continue
            old_tokens = _drop_trailing_commas(_tokens_in_span(old_tokens_by_line, old_span))
            new_tokens = _drop_trailing_commas(_tokens_in_span(new_tokens_by_line, new_span))
            if old_tokens == new_tokens:
                continue
            kept.append(new_span)
        if kept:
            ranges[file] = kept
    return ranges


def changed_ranges(base: str, pathspec: str = "*.rs") -> dict[str, list[tuple[int, int]]]:
    """Changed line ranges per file, restricted to `pathspec`, vs `base`.

    Excludes any hunk whose old and new sides are the same code once
    comments and incidental whitespace are gone: see `semantic_ranges`. The
    new side is read from the working tree, matching what the diff itself
    compares against (see the module docstring); the old side is read from
    `base`'s commit via `git show`, since the pre-change blob does not
    otherwise exist anywhere on disk. Either side missing -- a path the diff
    reports as changed but that no longer exists on disk, or a file that did
    not exist at `base` -- reads back as empty rather than raising, since
    there is nothing there to compare against.
    """
    base_sha = merge_base(base)
    result = subprocess.run(
        ["git", "diff", "--unified=0", "--no-prefix", base_sha, "--", pathspec],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    hunk_spans = parse_hunk_spans(result.stdout)

    def read_new(file: str) -> str:
        path = ROOT / file
        try:
            return path.read_text()
        except (FileNotFoundError, UnicodeDecodeError):
            return ""

    def read_old(file: str) -> str:
        blob = subprocess.run(
            ["git", "show", f"{base_sha}:{file}"],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        return blob.stdout if blob.returncode == 0 else ""

    return semantic_ranges(hunk_spans, read_old, read_new)


def cargo_bin_dir() -> Path:
    """Where `cargo install` puts binaries -- $CARGO_HOME/bin, or ~/.cargo/bin.

    Every gate tool is resolved against this exact path, never against
    `$PATH`. `$PATH` is not deterministic across machines: a name like
    `jscpd` can also be a leftover npm global, an old major version, or a
    system package, and a bare `shutil.which()` / `command -v` probe accepts
    whichever one happens to be sitting there first. Pinning the path pins
    the binary to the one this project just installed.
    """
    cargo_home = os.environ.get("CARGO_HOME")
    root = Path(cargo_home).expanduser() if cargo_home else Path.home() / ".cargo"
    return root / "bin"


def resolve_cargo_tool(name: str, on_install_failure_hint: str = "") -> Path:
    """The cargo-installed `name`, installing it first if it is missing.

    Never falls back to `$PATH`: see `cargo_bin_dir`. If cargo itself cannot
    produce the binary (offline, registry down, name typo), this fails loudly
    with `on_install_failure_hint` attached -- the place to name the actual
    reason this project cannot substitute some other build of the same name
    (a different language runtime, a different major version), since that
    reason has nowhere else durable to live.
    """
    binary = cargo_bin_dir() / name
    if not (binary.is_file() and os.access(binary, os.X_OK)):
        subprocess.run(["cargo", "install", name, "--locked"], cwd=ROOT)
    if not (binary.is_file() and os.access(binary, os.X_OK)):
        hint = f" {on_install_failure_hint}" if on_install_failure_hint else ""
        raise SystemExit(
            f"gate_diff: {name} is not at {binary} and `cargo install {name} "
            f"--locked` did not put it there.{hint}"
        )
    return binary


def intersects(a: tuple[int, int], b: tuple[int, int]) -> bool:
    """Whether two inclusive `(start, end)` line ranges overlap."""
    return a[0] <= b[1] and b[0] <= a[1]


def any_intersect(ranges: list[tuple[int, int]], span: tuple[int, int]) -> bool:
    return any(intersects(r, span) for r in ranges)
