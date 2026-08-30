#!/usr/bin/env python3
"""Unit tests for the diff-scoping logic the two quality gates share.

This is the part correctness actually hinges on: get the intersection wrong
and the gate either lets new debt through or fails an innocent PR for
something it never touched. Most of this runs against synthetic diff text and
synthetic tool output, needing neither git history nor
`rust-code-analysis-cli` / `jscpd` installed. The merge-base tests are the
exception: they build real, disposable git repositories under a temp
directory to reproduce a shallow checkout, because the bug they guard --
`git merge-base` failing on two disconnected single-commit histories -- only
exists in git's own history-walking behavior, not in anything this module
could fake convincingly.

    just test-quality-gates
    python3 scripts/ci/test_gate_diff.py
"""

from __future__ import annotations

import os
import stat
import subprocess
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


class LineHasCode(unittest.TestCase):
    def test_plain_code_line(self) -> None:
        self.assertEqual(gate_diff.line_has_code("let x = 1;"), [True])

    def test_blank_line(self) -> None:
        self.assertEqual(gate_diff.line_has_code(""), [False])
        self.assertEqual(gate_diff.line_has_code("   \t  "), [False])

    def test_pure_line_comment(self) -> None:
        self.assertEqual(gate_diff.line_has_code("// just a note"), [False])

    def test_code_with_trailing_comment_counts_as_code(self) -> None:
        self.assertEqual(gate_diff.line_has_code("let x = 1; // trailing"), [True])

    def test_slash_slash_inside_a_string_is_not_a_comment(self) -> None:
        text = 'let url = "https://example.com";'
        self.assertEqual(gate_diff.line_has_code(text), [True])

    def test_string_spanning_a_naive_comment_check_does_not_start_one(self) -> None:
        # A string literal containing `//` followed by a real comment on the
        # next line: the `//` inside the string must not be mistaken for the
        # start of a comment that swallows the rest of the string plus the
        # next, genuinely-commented, line.
        text = '\n'.join(['let s = "a // b";', "// this really is a comment"])
        self.assertEqual(gate_diff.line_has_code(text), [True, False])

    def test_single_line_block_comment(self) -> None:
        self.assertEqual(gate_diff.line_has_code("/* note */"), [False])

    def test_single_line_block_comment_with_trailing_code_counts_as_code(self) -> None:
        self.assertEqual(gate_diff.line_has_code("/* note */ let x = 1;"), [True])

    def test_multi_line_block_comment_is_all_blank(self) -> None:
        text = "\n".join(["/* start", "middle line", "end */"])
        self.assertEqual(gate_diff.line_has_code(text), [False, False, False])

    def test_multi_line_block_comment_with_code_before_and_after(self) -> None:
        text = "\n".join(["let a = 1; /* start", "middle line", "end */ let b = 2;"])
        self.assertEqual(gate_diff.line_has_code(text), [True, False, True])

    def test_nested_block_comments(self) -> None:
        # Rust block comments nest, unlike C's.
        text = "\n".join(["/* outer /* inner */ still commented", "*/ let x = 1;"])
        self.assertEqual(gate_diff.line_has_code(text), [False, True])

    def test_raw_string_containing_slashes_and_quotes(self) -> None:
        text = 'let s = r#"// not a comment, and "quoted" too"#;'
        self.assertEqual(gate_diff.line_has_code(text), [True])

    def test_raw_string_spanning_lines(self) -> None:
        text = "\n".join(['let s = r"line one', "// still string content", 'line three";'])
        self.assertEqual(gate_diff.line_has_code(text), [True, True, True])

    def test_raw_string_prefix_not_misdetected_mid_identifier(self) -> None:
        # `bar` ends in `r`; the `"` that follows belongs to a separate,
        # ordinary string literal, not a raw string starting at that `r`.
        text = 'let bar = 1; let s = "text";'
        self.assertEqual(gate_diff.line_has_code(text), [True])

    def test_char_literal_does_not_start_a_comment(self) -> None:
        text = "let c = '/';"
        self.assertEqual(gate_diff.line_has_code(text), [True])

    def test_escaped_char_literal(self) -> None:
        text = r"let c = '\n';"
        self.assertEqual(gate_diff.line_has_code(text), [True])

    def test_lifetime_is_not_treated_as_an_unterminated_char_literal(self) -> None:
        # If `'a` were treated as opening a char literal that never closes,
        # scanning would run past the real comment below looking for a
        # closing quote, hiding it from the result.
        text = "\n".join(["fn f<'a>(x: &'a str) -> &'a str { x }", "// a real comment"])
        self.assertEqual(gate_diff.line_has_code(text), [True, False])

    def test_string_containing_an_escaped_quote(self) -> None:
        text = r'let s = "she said \"hi\"";'
        self.assertEqual(gate_diff.line_has_code(text), [True])


class DropCommentOnlyRanges(unittest.TestCase):
    def test_hunk_that_is_entirely_comment_is_dropped(self) -> None:
        ranges = {"src/lib.rs": [(2, 2)]}
        files = {"src/lib.rs": "fn f() {\n    // just a comment\n}\n"}
        self.assertEqual(gate_diff.drop_comment_only_ranges(ranges, files.get), {})

    def test_hunk_with_any_real_code_line_is_kept_in_full(self) -> None:
        ranges = {"src/lib.rs": [(2, 3)]}
        files = {"src/lib.rs": "fn f() {\n    // a comment\n    let x = 1;\n}\n"}
        self.assertEqual(
            gate_diff.drop_comment_only_ranges(ranges, files.get),
            {"src/lib.rs": [(2, 3)]},
        )

    def test_only_the_comment_only_hunk_is_dropped_others_survive(self) -> None:
        ranges = {"src/lib.rs": [(2, 2), (4, 4)]}
        files = {"src/lib.rs": "fn f() {\n    // comment\n    let x = 1;\n    let y = 2;\n}\n"}
        self.assertEqual(
            gate_diff.drop_comment_only_ranges(ranges, files.get),
            {"src/lib.rs": [(4, 4)]},
        )

    def test_file_with_no_surviving_ranges_is_dropped_entirely(self) -> None:
        ranges = {"src/only_comments.rs": [(1, 1)]}
        files = {"src/only_comments.rs": "// nothing but this\n"}
        self.assertEqual(gate_diff.drop_comment_only_ranges(ranges, files.get), {})


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


class MergeBaseShallowHistory(unittest.TestCase):
    def _git(self, args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True)

    def _is_shallow(self, repo: Path) -> bool:
        return self._git(["rev-parse", "--is-shallow-repository"], repo).stdout.strip() == "true"

    def _commit(self, repo: Path, message: str) -> str:
        (repo / "file.txt").write_text(message)
        self._git(["add", "."], repo)
        self._git(["commit", "--quiet", "-m", message], repo)
        return self._git(["rev-parse", "HEAD"], repo).stdout.strip()

    def _init_repo(self, repo: Path, initial_branch: str) -> None:
        repo.mkdir()
        self._git(["init", "--quiet", "-b", initial_branch], repo)
        self._git(["config", "user.email", "test@example.com"], repo)
        self._git(["config", "user.name", "Test"], repo)

    def _shallow_checkout_of_branch_tip(self, origin: Path, branch: str, work: Path) -> None:
        """Reproduce what `actions/checkout` leaves behind for a PR head.

        A depth-1 `actions/checkout` configures the broad
        `+refs/heads/*:refs/remotes/origin/*` refspec but fetches only the
        one commit it needs, so the checked-out branch's remote-tracking ref
        exists locally with a single commit and no reachable parent -- the
        exact shape `ensure_ref`'s later fetch of a *different* branch does
        not, by itself, fix.
        """
        work.mkdir()
        self._git(["init", "--quiet"], work)
        self._git(["remote", "add", "origin", f"file://{origin}"], work)
        self._git(["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"], work)
        tip = self._git(["rev-parse", branch], origin).stdout.strip()
        self._git(
            ["fetch", "--quiet", "--depth=1", "origin", f"+{tip}:refs/remotes/origin/{branch}"],
            work,
        )
        self._git(["checkout", "--quiet", "-b", branch, f"origin/{branch}"], work)

    def test_deepens_shallow_history_to_find_the_merge_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            origin = Path(tmp) / "origin"
            work = Path(tmp) / "work"
            self._init_repo(origin, "main")
            shared_ancestor = self._commit(origin, "shared ancestor")
            self._git(["checkout", "--quiet", "-b", "feature"], origin)
            self._commit(origin, "feature work")
            self._git(["checkout", "--quiet", "main"], origin)
            self._commit(origin, "main moved on without the feature branch")

            self._shallow_checkout_of_branch_tip(origin, "feature", work)
            self.assertTrue(self._is_shallow(work))

            with mock.patch("gate_diff.ROOT", work):
                self.assertEqual(gate_diff.merge_base("origin/main"), shared_ancestor)
            self.assertFalse(self._is_shallow(work))

    def test_raises_when_histories_truly_share_no_ancestor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            origin_main = Path(tmp) / "origin_main"
            origin_feature = Path(tmp) / "origin_feature"
            work = Path(tmp) / "work"
            self._init_repo(origin_main, "main")
            self._commit(origin_main, "main's own unrelated root")
            self._init_repo(origin_feature, "feature")
            self._commit(origin_feature, "feature's own unrelated root")

            work.mkdir()
            self._git(["init", "--quiet"], work)
            self._git(["remote", "add", "origin", f"file://{origin_main}"], work)
            self._git(["fetch", "--quiet", "origin", "main"], work)
            self._git(["remote", "add", "elsewhere", f"file://{origin_feature}"], work)
            self._git(["fetch", "--quiet", "elsewhere", "feature"], work)
            self._git(["checkout", "--quiet", "-b", "feature", "elsewhere/feature"], work)
            self.assertFalse(self._is_shallow(work))

            with mock.patch("gate_diff.ROOT", work):
                with self.assertRaises(SystemExit) as raised:
                    gate_diff.merge_base("origin/main")
            self.assertIn("share no common ancestor", str(raised.exception))


class ChangedRangesExcludesCommentOnlyHunks(unittest.TestCase):
    """The end-to-end fixture: a real repo, a real over-limit function, and
    the exact `changed_ranges` pipeline `complexity_gate`/`duplication_gate`
    call in CI -- not a synthetic diff string standing in for one.
    """

    _OVER_LIMIT_FUNCTION = "\n".join(
        [
            "fn deeply_branching(x: i32) -> i32 {",
            "    if x == 0 { return 0; }",
            "    if x == 1 { return 1; }",
            "    if x == 2 { return 2; }",
            "    if x == 3 { return 3; }",
            "    if x == 4 { return 4; }",
            "    if x == 5 { return 5; }",
            "    if x == 6 { return 6; }",
            "    if x == 7 { return 7; }",
            "    if x == 8 { return 8; }",
            "    if x == 9 { return 9; }",
            "    x",
            "}",
        ]
    )

    def _git(self, args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True)

    def _init_repo_with_base_commit(self, repo: Path) -> None:
        repo.mkdir()
        self._git(["init", "--quiet", "-b", "main"], repo)
        self._git(["config", "user.email", "test@example.com"], repo)
        self._git(["config", "user.name", "Test"], repo)
        (repo / "src").mkdir()
        (repo / "src" / "lib.rs").write_text(self._OVER_LIMIT_FUNCTION + "\n")
        self._git(["add", "."], repo)
        self._git(["commit", "--quiet", "-m", "base"], repo)

    def _write_and_commit(self, repo: Path, contents: str, message: str) -> None:
        (repo / "src" / "lib.rs").write_text(contents)
        self._git(["add", "."], repo)
        self._git(["commit", "--quiet", "-m", message], repo)

    def test_removing_a_comment_inside_the_function_does_not_touch_it(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            self._init_repo_with_base_commit(repo)

            with_comment = self._OVER_LIMIT_FUNCTION.replace(
                "    x\n}", "    // fall through for anything else\n    x\n}"
            )
            self._write_and_commit(repo, with_comment + "\n", "add a comment")

            comment_removed = with_comment.replace(
                "    // fall through for anything else\n", ""
            )
            self._write_and_commit(repo, comment_removed + "\n", "remove only the comment")

            with mock.patch("gate_diff.ROOT", repo):
                ranges = gate_diff.changed_ranges("HEAD~1")

            self.assertEqual(
                ranges,
                {},
                "a hunk that only deleted a comment line must not touch anything",
            )

    def test_a_genuine_logic_edit_in_the_same_function_still_touches_it(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            self._init_repo_with_base_commit(repo)

            edited = self._OVER_LIMIT_FUNCTION.replace(
                "    if x == 9 { return 9; }", "    if x == 9 { return 90; }"
            )
            self._write_and_commit(repo, edited + "\n", "change a return value")

            with mock.patch("gate_diff.ROOT", repo):
                ranges = gate_diff.changed_ranges("HEAD~1")

            self.assertIn("src/lib.rs", ranges)
            touched = ranges["src/lib.rs"]
            self.assertTrue(
                gate_diff.any_intersect(touched, (1, 13)),
                f"a real logic edit must still land inside the function's span, got {touched}",
            )

    def test_mixed_hunk_of_comment_and_logic_still_touches_it(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            self._init_repo_with_base_commit(repo)

            edited = self._OVER_LIMIT_FUNCTION.replace(
                "    x\n}",
                "    // fall through for anything else\n    x + 1\n}",
            )
            self._write_and_commit(repo, edited + "\n", "comment plus a real edit, one hunk")

            with mock.patch("gate_diff.ROOT", repo):
                ranges = gate_diff.changed_ranges("HEAD~1")

            self.assertIn("src/lib.rs", ranges)
            self.assertTrue(gate_diff.any_intersect(ranges["src/lib.rs"], (1, 13)))


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
