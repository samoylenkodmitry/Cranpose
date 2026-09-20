#!/usr/bin/env python3
"""Fails on a function that is a second name for one call.

    pointless_wrapper_gate.py --base origin/main   # only what the diff adds
    pointless_wrapper_gate.py [<path> ...]         # everything, to clean up

Some functions are worth their signature however short the body is: a named
formula is supposed to be shorter than its name and its types.
`clamp01(value)` earns its place, and so does `lerp(start, end, t)`.

What does not earn a place is a function that passes its parameters straight
into one call and returns the result. It computes nothing, decides nothing
and names nothing new; it puts a second name in front of a call the caller
could have written, and a reader who meets the name has to go and look at it
to learn it meant nothing:

    fn winamp_window_modifier_for(config: WindowConfig) -> Modifier {
        Modifier::empty().window(config)
    }

So the test is not length. It is whether the body does anything besides
forward: no operator, no literal, no second call, and the arguments are the
parameters, unchanged.

Methods satisfying a trait are exempt -- `impl Trait for Type` fixes the
signature, so the body is not a choice -- and so is a default body in a
`trait` block, which is a fallback rather than a name. So is any function
with a `// forwards on purpose:` comment above it, for a name that is
load-bearing with the reason written after the colon: in a composable tree
an extra function is an extra recomposition scope, and then the forwarding
is the whole point. It is a comment rather than an attribute because this is
not a rustc lint, and `#[allow(..)]` of a lint the compiler has never heard
of is an error in its own right.

Prints one line per finding and exits 1 when it finds any.

With `--base`, only functions the diff adds are reported, the way
`complexity-gate` and `duplication-gate` work: a repository that already has
some cannot adopt a gate that fails on all of them at once, and a gate that
has to be switched off is not a gate.
"""

import re
import subprocess
import sys
from pathlib import Path

FUNCTION = re.compile(
    r"^(?P<indent> *)(?:pub(?:\([^)]*\))? )?(?:async )?fn (?P<name>\w+)"
)
IMPL_FOR = re.compile(r"^ *(?:unsafe )?impl\b.*\bfor\b.*\{")
TRAIT_BLOCK = re.compile(r"^ *(?:pub(?:\([^)]*\))? )?(?:unsafe )?trait\b.*\{")
BODY_KEYWORD = re.compile(r"^\s*(let|if|match|for|while|loop|return|unsafe|\}|//|#\[)")
# One call and nothing else: a path or receiver chain, then one argument list.
FORWARDING = re.compile(r"^(?P<callee>[A-Za-z_][\w:.]*)\((?P<args>[^()]*)\);?$")
OPERATORS = re.compile(r"[+\-*/%<>!&|^?]|==|\bas\b")
EXEMPT = "forwards on purpose:"


def balanced_end(lines, start):
    depth = 0
    for index in range(start, len(lines)):
        depth += lines[index].count("{") - lines[index].count("}")
        if depth <= 0 and (index > start or "{" in lines[index]):
            return index + 1
    return len(lines)


def trait_impl_spans(lines):
    """Line ranges where a signature is somebody else's to choose."""
    return [
        (index, balanced_end(lines, index))
        for index, line in enumerate(lines)
        if IMPL_FOR.match(line) or TRAIT_BLOCK.match(line)
    ]


def parameter_names(signature):
    inside = signature[signature.index("(") + 1 : signature.rindex(")")]
    names = []
    depth = 0
    current = ""
    for char in inside:
        depth += char in "<(["
        depth -= char in ">)]"
        if char == "," and depth == 0:
            names.append(current)
            current = ""
        else:
            current += char
    names.append(current)
    cleaned = []
    for name in names:
        name = name.strip()
        if not name or name.startswith("&") or name in {"self", "&self", "&mut self"}:
            continue
        cleaned.append(name.split(":")[0].strip().lstrip("mut "))
    return [name for name in cleaned if name]


def signature_and_body(lines, start):
    open_index = start
    while open_index < len(lines) and not lines[open_index].rstrip().endswith("{"):
        if lines[open_index].rstrip().endswith(";"):
            return None
        open_index += 1
        if open_index - start > 12:
            return None
    if open_index >= len(lines):
        return None
    end = balanced_end(lines, start)
    body = lines[open_index + 1 : end - 1]
    if not body or any(BODY_KEYWORD.match(line) for line in body):
        return None
    signature = " ".join(line.strip() for line in lines[start : open_index + 1])
    return signature.rstrip("{ ").strip(), " ".join(line.strip() for line in body)


def only_forwards(signature, body):
    match = FORWARDING.match(body)
    if not match:
        return False
    callee = match.group("callee")
    if OPERATORS.search(callee):
        return False
    # An accessor reaches into the receiver, and a constructor shorthand names
    # its own type. Neither is a second name for somebody else's call.
    if callee.startswith("self.") or callee.startswith("Self::"):
        return False
    arguments = [
        argument.strip() for argument in match.group("args").split(",") if argument.strip()
    ]
    if any(OPERATORS.search(argument) or "(" in argument for argument in arguments):
        return False
    parameters = parameter_names(signature)
    passed = [argument.lstrip("&").removeprefix("mut ").strip() for argument in arguments]
    return sorted(passed) == sorted(parameters)


def exempted(lines, start):
    """Whether the attributes above the function carry the opt-out. It may sit
    anywhere in the run of attributes and comments, not only on the line
    immediately above."""
    index = start - 1
    while index >= 0:
        line = lines[index].strip()
        if not (line.startswith("#[") or line.startswith("//") or not line):
            return False
        if EXEMPT in line:
            return True
        index -= 1
    return False


def findings(path):
    lines = path.read_text(encoding="utf-8").splitlines()
    spans = trait_impl_spans(lines)
    for index, line in enumerate(lines):
        declared = FUNCTION.match(line)
        # `fn main` has the name the platform looks for, whatever it forwards to.
        if not declared or declared.group("name") == "main":
            continue
        if any(start <= index < end for start, end in spans):
            continue
        if exempted(lines, index):
            continue
        parts = signature_and_body(lines, index)
        if parts and only_forwards(*parts):
            yield index + 1, parts[0], parts[1]


def added_lines(base):
    """{path: {line numbers the diff against `base` adds}}"""
    diff = subprocess.run(
        ["git", "diff", "--unified=0", f"{base}...HEAD", "--", "*.rs"],
        capture_output=True,
        text=True,
        check=False,
    )
    if diff.returncode != 0:
        print(f"pointless_wrapper_gate: no diff against {base}; checking everything")
        return None
    added = {}
    path = None
    line = 0
    for row in diff.stdout.splitlines():
        if row.startswith("+++ b/"):
            path = row[6:]
            added.setdefault(path, set())
        elif row.startswith("@@"):
            hunk = re.search(r"\+(\d+)", row)
            line = int(hunk.group(1)) if hunk else 0
        elif row.startswith("+") and path:
            added[path].add(line)
            line += 1
    return added


def main(paths):
    base = None
    if paths and paths[0] == "--base":
        base = paths[1] if len(paths) > 1 else "origin/main"
        paths = paths[2:]
    added = added_lines(base) if base else None
    roots = [Path(path) for path in paths] or [Path("crates"), Path("apps")]
    found = 0
    for root in roots:
        files = sorted(root.rglob("*.rs")) if root.is_dir() else [root]
        for path in files:
            if "target" in path.parts:
                continue
            touched = None if added is None else added.get(str(path))
            if added is not None and not touched:
                continue
            for line, signature, body in findings(path):
                if touched is not None and line not in touched:
                    continue
                found += 1
                print(f"{path}:{line}: `{signature}` only forwards to `{body}`")
    if found:
        print(f"{found} function(s) are a second name for one call")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
