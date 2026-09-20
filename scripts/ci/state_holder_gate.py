#!/usr/bin/env python3
"""Fails on a state holder rebuilt on every pass.

    state_holder_gate.py [<path> ...]

A composable that needs several pieces of state gathers them into a small
`Copy` struct of handles. Written a field at a time it looks right and is not:

    fn rememberWindowState(width: f32, height: f32) -> WindowState {
        WindowState {
            position: rememberMutableStateOf(|| None),
            size: rememberMutableStateOf(move || Size::new(width, height)),
        }
    }

Each field remembers its own handle, so the values survive -- but the struct
around them does not. It is built again on every pass, out of one slot per
field, and the name says `remember` about an object nothing remembers.

Remember the struct itself instead, the way Jetpack Compose does -- one
slot, built once, and later passes read the one that is already there:

    impl WindowState {
        fn new(width: f32, height: f32) -> Self {
            WindowState {
                position: mutableStateOf(None),
                size: mutableStateOf(Size::new(width, height)),
            }
        }
    }

    remember(move || WindowState::new(width, height)).with(|state| *state)

A state made while a `remember` is building its value belongs to that slot
and is released with it, so this neither leaks nor loses anything.

So the test is a struct-literal field initialised straight from a
`remember...` call. A `let` binding is not this shape: a local handle is the
value itself, and nothing is built around it.

Unlike `complexity-gate` and the rest, this one reads the whole tree rather
than the diff. Those gates are diff-based because the tree still has what
they look for; this one has none left, and a gate with nothing to grandfather
should not leave the door open behind it.

Prints one line per finding and exits 1 when it finds any.
"""

import re
import sys
from pathlib import Path

FIELD_FROM_REMEMBER = re.compile(
    r"^\s*(?P<field>\w+)\s*:\s*(?:[\w:]+::)?(?P<hook>remember(?:MutableStateOf"
    r"(?:NeverEqual)?|UpdatedState|Keyed))\s*\("
)


def findings(path):
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        found = FIELD_FROM_REMEMBER.match(line)
        if found:
            yield number, found.group("field"), found.group("hook")


def main(paths):
    roots = [Path(path) for path in paths] or [Path("crates"), Path("apps")]
    found = 0
    for root in roots:
        files = sorted(root.rglob("*.rs")) if root.is_dir() else [root]
        for path in files:
            if "target" in path.parts:
                continue
            for line, field, hook in findings(path):
                found += 1
                print(f"{path}:{line}: field `{field}` is built from `{hook}`")
    if found:
        print(f"{found} struct field(s) rebuild a state holder every pass")
        print("Remember the holder itself: remember(|| Holder { .. mutableStateOf(..) }).")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
