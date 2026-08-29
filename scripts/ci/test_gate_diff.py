#!/usr/bin/env python3
"""Unit tests for the diff-scoping logic the two quality gates share.

This is the part correctness actually hinges on: get the intersection wrong
and the gate either lets new debt through or fails an innocent PR for
something it never touched. Runs against synthetic diff text and synthetic
tool output, so it needs neither git history nor `rust-code-analysis-cli` /
`jscpd` installed.

    just test-quality-gates
    python3 scripts/ci/test_gate_diff.py
"""

from __future__ import annotations

import os
import stat
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import complexity_gate
import duplication_gate
import gate_diff


class ParseUnifiedDiffZeroContext(unittest.TestCase):
    def test_single_hunk_modified_file(self) -> None:
        diff = "\n".join(
            [
                "diff --git a/src/lib.rs b/src/lib.rs",
                "--- src/lib.rs",
                "+++ src/lib.rs",
                "@@ -10,2 +10,3 @@",
                "+one",
                "+two",
                "+three",
            ]
        )
        self.assertEqual(
            gate_diff.parse_unified_diff_zero_context(diff),
            {"src/lib.rs": [(10, 12)]},
        )

    def test_single_added_line_omits_count(self) -> None:
        # unified diff omits the trailing ",1" when a hunk covers one line.
        diff = "\n".join(
            [
                "--- src/lib.rs",
                "+++ src/lib.rs",
                "@@ -5 +5 @@",
                "-old",
                "+new",
            ]
        )
        self.assertEqual(
            gate_diff.parse_unified_diff_zero_context(diff),
            {"src/lib.rs": [(5, 5)]},
        )

    def test_pure_deletion_hunk_has_no_new_side_range(self) -> None:
        diff = "\n".join(
            [
                "--- src/lib.rs",
                "+++ src/lib.rs",
                "@@ -20,3 +19,0 @@",
                "-gone",
                "-gone too",
                "-and this",
            ]
        )
        self.assertEqual(gate_diff.parse_unified_diff_zero_context(diff), {})

    def test_deleted_file_is_skipped(self) -> None:
        diff = "\n".join(
            [
                "--- src/dead.rs",
                "+++ /dev/null",
                "@@ -1,3 +0,0 @@",
                "-a",
                "-b",
                "-c",
            ]
        )
        self.assertEqual(gate_diff.parse_unified_diff_zero_context(diff), {})

    def test_multiple_hunks_and_files(self) -> None:
        diff = "\n".join(
            [
                "--- src/a.rs",
                "+++ src/a.rs",
                "@@ -1,0 +1,2 @@",
                "+a1",
                "+a2",
                "@@ -50,0 +52,1 @@",
                "+a3",
                "--- src/b.rs",
                "+++ src/b.rs",
                "@@ -3,1 +3,1 @@",
                "-old",
                "+new",
            ]
        )
        self.assertEqual(
            gate_diff.parse_unified_diff_zero_context(diff),
            {"src/a.rs": [(1, 2), (52, 52)], "src/b.rs": [(3, 3)]},
        )


class Intersects(unittest.TestCase):
    def test_overlapping_ranges(self) -> None:
        self.assertTrue(gate_diff.intersects((10, 20), (15, 25)))
        self.assertTrue(gate_diff.intersects((15, 25), (10, 20)))

    def test_touching_at_a_single_line_counts_as_overlap(self) -> None:
        self.assertTrue(gate_diff.intersects((10, 20), (20, 30)))

    def test_disjoint_ranges(self) -> None:
        self.assertFalse(gate_diff.intersects((10, 20), (21, 30)))

    def test_one_range_contains_the_other(self) -> None:
        self.assertTrue(gate_diff.intersects((1, 100), (40, 41)))

    def test_any_intersect_true_only_when_one_range_matches(self) -> None:
        ranges = [(1, 5), (50, 60)]
        self.assertTrue(gate_diff.any_intersect(ranges, (55, 58)))
        self.assertFalse(gate_diff.any_intersect(ranges, (10, 20)))
        self.assertFalse(gate_diff.any_intersect([], (1, 1000)))


class ResolveCargoTool(unittest.TestCase):
    def test_cargo_bin_dir_honors_cargo_home(self) -> None:
        with mock.patch.dict(os.environ, {"CARGO_HOME": "/scratch/cargo"}):
            self.assertEqual(gate_diff.cargo_bin_dir(), Path("/scratch/cargo/bin"))

    def test_cargo_bin_dir_falls_back_to_dot_cargo(self) -> None:
        env_without_cargo_home = {k: v for k, v in os.environ.items() if k != "CARGO_HOME"}
        with mock.patch.dict(os.environ, env_without_cargo_home, clear=True):
            self.assertEqual(gate_diff.cargo_bin_dir(), Path.home() / ".cargo" / "bin")

    def test_never_consults_path_when_already_at_the_pinned_location(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            fake_tool = bin_dir / "some-tool"
            fake_tool.write_text("#!/bin/sh\n")
            fake_tool.chmod(fake_tool.stat().st_mode | stat.S_IEXEC)

            with mock.patch.dict(os.environ, {"CARGO_HOME": tmp}):
                with mock.patch("gate_diff.subprocess.run") as run:
                    resolved = gate_diff.resolve_cargo_tool("some-tool")
                    run.assert_not_called()
            self.assertEqual(resolved, fake_tool)

    def test_installs_when_missing_then_resolves_to_the_pinned_location(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            fake_tool = bin_dir / "some-tool"

            def fake_install(args, cwd=None):
                fake_tool.write_text("#!/bin/sh\n")
                fake_tool.chmod(fake_tool.stat().st_mode | stat.S_IEXEC)

            with mock.patch.dict(os.environ, {"CARGO_HOME": tmp}):
                with mock.patch("gate_diff.subprocess.run", side_effect=fake_install) as run:
                    resolved = gate_diff.resolve_cargo_tool("some-tool")
                    run.assert_called_once()
            self.assertEqual(resolved, fake_tool)

    def test_raises_with_the_hint_when_install_does_not_produce_the_binary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "bin").mkdir()
            with mock.patch.dict(os.environ, {"CARGO_HOME": tmp}):
                with mock.patch("gate_diff.subprocess.run"):
                    with self.assertRaises(SystemExit) as raised:
                        gate_diff.resolve_cargo_tool("some-tool", "do not substitute npm")
            self.assertIn("do not substitute npm", str(raised.exception))


class ComplexityFindViolations(unittest.TestCase):
    def test_flags_only_functions_the_diff_touches(self) -> None:
        ranges = {"src/lib.rs": [(10, 15)]}
        functions_by_file = {
            "src/lib.rs": [
                {"name": "touched_and_complex", "start": 10, "end": 15, "cyclomatic": 25},
                {"name": "untouched_and_complex", "start": 100, "end": 120, "cyclomatic": 99},
                {"name": "touched_but_simple", "start": 12, "end": 13, "cyclomatic": 3},
            ]
        }
        violations = complexity_gate.find_violations(ranges, functions_by_file, max_cyclomatic=20)
        self.assertEqual(len(violations), 1)
        self.assertIn("touched_and_complex", violations[0])
        self.assertIn("cyclomatic complexity 25", violations[0])

    def test_no_violations_when_nothing_over_the_limit(self) -> None:
        ranges = {"src/lib.rs": [(1, 100)]}
        functions_by_file = {
            "src/lib.rs": [{"name": "fine", "start": 1, "end": 10, "cyclomatic": 5}]
        }
        self.assertEqual(
            complexity_gate.find_violations(ranges, functions_by_file, max_cyclomatic=20), []
        )

    def test_untouched_file_contributes_no_violations(self) -> None:
        ranges = {"src/other.rs": [(1, 5)]}
        functions_by_file = {
            "src/lib.rs": [{"name": "huge", "start": 1, "end": 500, "cyclomatic": 500}]
        }
        self.assertEqual(
            complexity_gate.find_violations(ranges, functions_by_file, max_cyclomatic=20), []
        )


class DuplicationFindViolations(unittest.TestCase):
    def _dup(self, first: tuple[str, int, int], second: tuple[str, int, int], lines: int = 12) -> dict:
        return {
            "firstFile": {"name": first[0], "start": first[1], "end": first[2]},
            "secondFile": {"name": second[0], "start": second[1], "end": second[2]},
            "lines": lines,
        }

    def test_new_code_duplicating_old_code_fails(self) -> None:
        ranges = {"src/new.rs": [(1, 20)]}
        dup = self._dup(("src/new.rs", 5, 16), ("src/old.rs", 100, 111))
        violations = duplication_gate.find_violations([dup], ranges)
        self.assertEqual(len(violations), 1)
        self.assertIn("src/new.rs:5-16 (new)", violations[0])
        self.assertNotIn("src/old.rs:100-111 (new)", violations[0])

    def test_two_untouched_clones_are_not_flagged(self) -> None:
        ranges = {"src/elsewhere.rs": [(1, 5)]}
        dup = self._dup(("src/old_a.rs", 1, 12), ("src/old_b.rs", 1, 12))
        self.assertEqual(duplication_gate.find_violations([dup], ranges), [])

    def test_new_code_duplicating_itself_flags_both_sides(self) -> None:
        ranges = {"src/new.rs": [(1, 50)]}
        dup = self._dup(("src/new.rs", 1, 12), ("src/new.rs", 20, 31))
        violations = duplication_gate.find_violations([dup], ranges)
        self.assertEqual(len(violations), 1)
        self.assertIn("(new)", violations[0])


if __name__ == "__main__":
    unittest.main()
