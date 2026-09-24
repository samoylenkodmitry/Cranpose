#!/usr/bin/env python3
"""move_inline_tests.py keeps string literals intact while it moves a module.

Both cases are regressions from moving #759's modules: a raw string's
indentation was stripped with the code around it, which changed test
fixtures whose indentation is data, and a `}` at the start of a raw string's
line was taken for the end of the module.
"""
import importlib.util
import pathlib
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("move_inline_tests", HERE / "move_inline_tests.py")
mover = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mover)

SOURCE = '''pub fn value() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHADER: &str = r#"
@vertex
fn main() {
}
    indented data
"#;

    const JOINED: &str = "one \\
        two";

    #[test]
    fn reads() {
        assert_eq!(value(), 1);
        let _quote = '"';
    }
}
'''

MOVED = '''use super::*;

const SHADER: &str = r#"
@vertex
fn main() {
}
    indented data
"#;

const JOINED: &str = "one \\
        two";

#[test]
fn reads() {
    assert_eq!(value(), 1);
    let _quote = '"';
}
'''

IMPLEMENTATION = '''pub fn value() -> u8 {
    1
}

#[cfg(test)]
#[path = "tests/probe_tests.rs"]
mod tests;
'''

failures = []
with tempfile.TemporaryDirectory() as directory:
    source = pathlib.Path(directory) / "probe.rs"
    source.write_text(SOURCE)
    moved = mover.split_out(source, replace=False)
    target = pathlib.Path(directory) / "tests" / "probe_tests.rs"
    if moved != 1:
        failures.append(f"moved {moved} modules, expected 1")
    if not target.exists():
        failures.append("the tests file was not written")
    elif target.read_text() != MOVED:
        failures.append(f"moved module differs:\n{target.read_text()}")
    if source.read_text() != IMPLEMENTATION:
        failures.append(f"implementation differs:\n{source.read_text()}")

if failures:
    for failure in failures:
        print(f"not ok: {failure}")
    sys.exit(1)
print("ok: string literals keep their contents and do not end the module")
