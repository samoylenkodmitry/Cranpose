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


class ParseHunkSpans(unittest.TestCase):
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
            gate_diff.parse_hunk_spans(diff),
            {"src/lib.rs": [((10, 11), (10, 12))]},
        )

    def test_single_line_hunk_omits_count(self) -> None:
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
            gate_diff.parse_hunk_spans(diff),
            {"src/lib.rs": [((5, 5), (5, 5))]},
        )

    def test_pure_deletion_hunk_has_no_new_side_span(self) -> None:
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
        self.assertEqual(
            gate_diff.parse_hunk_spans(diff),
            {"src/lib.rs": [((20, 22), None)]},
        )

    def test_pure_addition_hunk_has_no_old_side_span(self) -> None:
        diff = "\n".join(
            [
                "--- src/lib.rs",
                "+++ src/lib.rs",
                "@@ -50,0 +52,1 @@",
                "+a3",
            ]
        )
        self.assertEqual(
            gate_diff.parse_hunk_spans(diff),
            {"src/lib.rs": [(None, (52, 52))]},
        )

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
        self.assertEqual(gate_diff.parse_hunk_spans(diff), {})

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
            gate_diff.parse_hunk_spans(diff),
            {
                "src/a.rs": [(None, (1, 2)), (None, (52, 52))],
                "src/b.rs": [((3, 3), (3, 3))],
            },
        )


class CodeTokensByLine(unittest.TestCase):
    def test_plain_code_line(self) -> None:
        self.assertEqual(
            gate_diff.code_tokens_by_line("let x = 1;"),
            [["let", "x", "=", "1", ";"]],
        )

    def test_blank_line(self) -> None:
        self.assertEqual(gate_diff.code_tokens_by_line(""), [[]])
        self.assertEqual(gate_diff.code_tokens_by_line("   \t  "), [[]])

    def test_pure_line_comment(self) -> None:
        self.assertEqual(gate_diff.code_tokens_by_line("// just a note"), [[]])

    def test_code_with_trailing_comment_yields_only_the_codes_tokens(self) -> None:
        self.assertEqual(
            gate_diff.code_tokens_by_line("let x = 1; // trailing"),
            [["let", "x", "=", "1", ";"]],
        )

    def test_slash_slash_inside_a_string_is_one_string_token_not_a_comment(self) -> None:
        text = 'let url = "https://example.com";'
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "url", "=", '"https://example.com"', ";"]],
        )

    def test_string_spanning_a_naive_comment_check_does_not_start_one(self) -> None:
        # A string literal containing `//` followed by a real comment on the
        # next line: the `//` inside the string must not be mistaken for the
        # start of a comment that swallows the rest of the string plus the
        # next, genuinely-commented, line.
        text = "\n".join(['let s = "a // b";', "// this really is a comment"])
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "s", "=", '"a // b"', ";"], []],
        )

    def test_single_line_block_comment(self) -> None:
        self.assertEqual(gate_diff.code_tokens_by_line("/* note */"), [[]])

    def test_single_line_block_comment_with_trailing_code(self) -> None:
        self.assertEqual(
            gate_diff.code_tokens_by_line("/* note */ let x = 1;"),
            [["let", "x", "=", "1", ";"]],
        )

    def test_multi_line_block_comment_is_all_blank(self) -> None:
        text = "\n".join(["/* start", "middle line", "end */"])
        self.assertEqual(gate_diff.code_tokens_by_line(text), [[], [], []])

    def test_multi_line_block_comment_with_code_before_and_after(self) -> None:
        text = "\n".join(["let a = 1; /* start", "middle line", "end */ let b = 2;"])
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "a", "=", "1", ";"], [], ["let", "b", "=", "2", ";"]],
        )

    def test_nested_block_comments(self) -> None:
        # Rust block comments nest, unlike C's.
        text = "\n".join(["/* outer /* inner */ still commented", "*/ let x = 1;"])
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [[], ["let", "x", "=", "1", ";"]],
        )

    def test_raw_string_containing_slashes_and_quotes_is_one_token(self) -> None:
        text = 'let s = r#"// not a comment, and "quoted" too"#;'
        raw_token = 'r#"// not a comment, and "quoted" too"#'
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "s", "=", raw_token, ";"]],
        )

    def test_raw_string_spanning_lines_is_one_token_on_its_start_line(self) -> None:
        text = "\n".join(['let s = r"line one', "// still string content", 'line three";'])
        raw_token = 'r"line one\n// still string content\nline three"'
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "s", "=", raw_token], [], [";"]],
        )

    def test_raw_string_prefix_not_misdetected_mid_identifier(self) -> None:
        # `bar` ends in `r`; the `"` that follows belongs to a separate,
        # ordinary string literal, not a raw string starting at that `r`.
        text = 'let bar = 1; let s = "text";'
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "bar", "=", "1", ";", "let", "s", "=", '"text"', ";"]],
        )

    def test_char_literal_does_not_start_a_comment(self) -> None:
        text = "let c = '/';"
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "c", "=", "'/'", ";"]],
        )

    def test_escaped_char_literal(self) -> None:
        text = r"let c = '\n';"
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "c", "=", r"'\n'", ";"]],
        )

    def test_lifetime_is_not_treated_as_an_unterminated_char_literal(self) -> None:
        # If `'a` were treated as opening a char literal that never closes,
        # scanning would run past the real comment below looking for a
        # closing quote, hiding it from the result. The exact token
        # breakdown of the lifetime-heavy line is not the point (each bare
        # `'` and the identifier after it fall out as separate tokens); what
        # matters is that it terminates and the comment on the next line is
        # correctly seen as holding no code at all.
        text = "\n".join(["fn f<'a>(x: &'a str) -> &'a str { x }", "// a real comment"])
        tokens = gate_diff.code_tokens_by_line(text)
        self.assertTrue(tokens[0], "the code line must yield at least one token")
        self.assertEqual(tokens[1], [])

    def test_string_containing_an_escaped_quote_is_one_token(self) -> None:
        text = r'let s = "she said \"hi\"";'
        string_token = r'"she said \"hi\""'
        self.assertEqual(
            gate_diff.code_tokens_by_line(text),
            [["let", "s", "=", string_token, ";"]],
        )


class DropTrailingCommas(unittest.TestCase):
    def test_comma_before_closing_paren_is_dropped(self) -> None:
        self.assertEqual(
            gate_diff._drop_trailing_commas(["f", "(", "a", ",", ")"]),
            ["f", "(", "a", ")"],
        )

    def test_comma_before_closing_bracket_is_dropped(self) -> None:
        self.assertEqual(
            gate_diff._drop_trailing_commas(["[", "1", ",", "]"]),
            ["[", "1", "]"],
        )

    def test_comma_before_closing_brace_is_dropped(self) -> None:
        self.assertEqual(
            gate_diff._drop_trailing_commas(["{", "x", ":", "1", ",", "}"]),
            ["{", "x", ":", "1", "}"],
        )

    def test_comma_between_arguments_is_kept(self) -> None:
        self.assertEqual(
            gate_diff._drop_trailing_commas(["f", "(", "a", ",", "b", ")"]),
            ["f", "(", "a", ",", "b", ")"],
        )

    def test_trailing_comma_at_the_very_end_of_the_list_is_kept(self) -> None:
        # No token follows it at all, so there is no closing delimiter to
        # confirm it precedes -- the conservative, safe default is to leave
        # it as a real token rather than guess.
        self.assertEqual(gate_diff._drop_trailing_commas(["a", ","]), ["a", ","])


class SemanticRanges(unittest.TestCase):
    def test_comment_only_edit_is_dropped(self) -> None:
        hunk_spans = {"src/lib.rs": [((2, 2), (2, 2))]}
        old = {"src/lib.rs": "fn f() {\n    let x = 1;  // set x\n}\n"}
        new = {"src/lib.rs": "fn f() {\n    let x = 1;\n}\n"}
        self.assertEqual(gate_diff.semantic_ranges(hunk_spans, old.get, new.get), {})

    def test_hunk_with_a_real_code_change_is_kept_in_full(self) -> None:
        hunk_spans = {"src/lib.rs": [((2, 3), (2, 3))]}
        old = {"src/lib.rs": "fn f() {\n    // a comment\n    let x = 0;\n}\n"}
        new = {"src/lib.rs": "fn f() {\n    // updated comment\n    let x = 1;\n}\n"}
        self.assertEqual(
            gate_diff.semantic_ranges(hunk_spans, old.get, new.get),
            {"src/lib.rs": [(2, 3)]},
        )

    def test_only_the_semantically_unchanged_hunk_is_dropped_others_survive(self) -> None:
        hunk_spans = {"src/lib.rs": [((2, 2), (2, 2)), ((4, 4), (4, 4))]}
        old = {"src/lib.rs": "fn f() {\n    // comment\n    let x = 1;\n    let y = 1;\n}\n"}
        new = {"src/lib.rs": "fn f() {\n    // different comment\n    let x = 1;\n    let y = 2;\n}\n"}
        self.assertEqual(
            gate_diff.semantic_ranges(hunk_spans, old.get, new.get),
            {"src/lib.rs": [(4, 4)]},
        )

    def test_file_with_no_surviving_ranges_is_dropped_entirely(self) -> None:
        hunk_spans = {"src/only_comments.rs": [((1, 1), (1, 1))]}
        old = {"src/only_comments.rs": "// nothing but this\n"}
        new = {"src/only_comments.rs": "// nothing but this, reworded\n"}
        self.assertEqual(gate_diff.semantic_ranges(hunk_spans, old.get, new.get), {})

    def test_pure_deletion_hunk_contributes_no_range_regardless_of_content(self) -> None:
        hunk_spans = {"src/lib.rs": [((5, 7), None)]}
        old = {"src/lib.rs": "fn f() {\n    let x = 1;\n    let y = 2;\n    let z = 3;\n}\n"}
        new = {"src/lib.rs": "fn f() {\n}\n"}
        self.assertEqual(gate_diff.semantic_ranges(hunk_spans, old.get, new.get), {})

    def test_comment_removal_that_collapses_a_block_onto_one_line_is_dropped(self) -> None:
        # The mechanism actually found in #540's diff: an `if` block whose
        # only content is a comment collapses, once the comment is deleted
        # and the formatter merges the now-empty block, from three lines to
        # one (`if cond {\n    // ...\n}` -> `if cond {}`). The surviving
        # line still contains code bytes (`if cond {}`), so a check that
        # only asked "does the new side have code" would keep this hunk in
        # scope for no reason -- the fix has to compare old and new tokens,
        # not just ask whether the new side is nonempty.
        hunk_spans = {"src/lib.rs": [((2, 4), (2, 2))]}
        old = {
            "src/lib.rs": "\n".join(
                [
                    "fn f(cond: bool) {",
                    "    if cond {",
                    "        // Found something",
                    "    }",
                    "}",
                    "",
                ]
            )
        }
        new = {
            "src/lib.rs": "\n".join(
                [
                    "fn f(cond: bool) {",
                    "    if cond {}",
                    "}",
                    "",
                ]
            )
        }
        self.assertEqual(gate_diff.semantic_ranges(hunk_spans, old.get, new.get), {})

    def test_whitespace_reorder_bundled_with_a_real_token_change_is_kept(self) -> None:
        # Pinned safety property: normalizing away comments/whitespace must
        # never mask an actual token-level change riding along with a
        # reformat.
        hunk_spans = {"src/lib.rs": [((1, 1), (1, 2))]}
        old = {"src/lib.rs": "let x = 1;\n"}
        new = {"src/lib.rs": "let   x =\n    2;\n"}
        self.assertEqual(
            gate_diff.semantic_ranges(hunk_spans, old.get, new.get),
            {"src/lib.rs": [(1, 2)]},
        )

    def test_string_literal_content_is_compared_not_discarded(self) -> None:
        # Pinned safety property: a string literal that itself contains
        # comment-like or whitespace-significant text is a single atomic
        # token, never split or normalized internally -- so two different
        # string values are never mistaken for the same code just because
        # both superficially resemble a comment.
        hunk_spans = {"src/lib.rs": [((1, 1), (1, 1))]}
        old = {"src/lib.rs": 'let s = "// keep me A";\n'}
        new = {"src/lib.rs": 'let s = "// keep me B";\n'}
        self.assertEqual(
            gate_diff.semantic_ranges(hunk_spans, old.get, new.get),
            {"src/lib.rs": [(1, 1)]},
        )

    def test_call_reformatted_onto_fewer_lines_drops_only_its_trailing_comma(self) -> None:
        # The exact mechanism found in #540's diff (e.g. `tab_bar.rs`): a
        # leading comment on one call argument holds the call multi-line;
        # once it is deleted, rustfmt's line-length heuristic reflows the
        # call, which also removes the trailing comma before the closing
        # paren that a multi-line call gets and a single-line one does not.
        # Nothing about the call's arguments changed.
        hunk_spans = {"src/lib.rs": [((1, 5), (1, 1))]}
        old = {
            "src/lib.rs": "\n".join(
                [
                    "f(",
                    "    // pick the material",
                    "    material(),",
                    "    move || dynamics(),",
                    ");",
                    "",
                ]
            )
        }
        new = {"src/lib.rs": "f(material(), move || dynamics());\n"}
        self.assertEqual(gate_diff.semantic_ranges(hunk_spans, old.get, new.get), {})

    def test_trailing_comma_change_bundled_with_a_real_edit_is_still_kept(self) -> None:
        # Pinned safety property: dropping one grammar-insignificant comma
        # must never mask an actual argument change riding along with it.
        hunk_spans = {"src/lib.rs": [((1, 1), (1, 1))]}
        old = {"src/lib.rs": "f(a, b,);\n"}
        new = {"src/lib.rs": "f(a, c);\n"}
        self.assertEqual(
            gate_diff.semantic_ranges(hunk_spans, old.get, new.get),
            {"src/lib.rs": [(1, 1)]},
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


class ChangedRangesExcludesSemanticallyUnchangedHunks(unittest.TestCase):
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

    def test_comment_deletion_that_collapses_a_branch_onto_one_line_does_not_touch_it(
        self,
    ) -> None:
        # This is the pattern #540's real diff actually hits: a branch whose
        # body is only a comment collapses from multiple lines to one empty
        # block once the comment goes and a format/clippy pass merges the
        # braces. The surviving line still contains code bytes (`{}`), so a
        # rule that only asked "does the new side have code" -- the first,
        # too-narrow fix -- kept this hunk in scope for no reason.
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            self._init_repo_with_base_commit(repo)

            with_comment_block = self._OVER_LIMIT_FUNCTION.replace(
                "    if x == 9 { return 9; }",
                "\n".join(
                    [
                        "    if x == 9 {",
                        "        // nothing special about nine",
                        "    }",
                    ]
                ),
            )
            self._write_and_commit(repo, with_comment_block + "\n", "expand nine's branch")

            collapsed = self._OVER_LIMIT_FUNCTION.replace(
                "    if x == 9 { return 9; }", "    if x == 9 {}"
            )
            self._write_and_commit(repo, collapsed + "\n", "delete the comment, collapse the block")

            with mock.patch("gate_diff.ROOT", repo):
                ranges = gate_diff.changed_ranges("HEAD~1")

            self.assertEqual(
                ranges,
                {},
                "deleting a comment and collapsing its now-empty block is not a "
                f"logic edit, got {ranges}",
            )

    def test_reformatting_a_line_while_also_changing_it_still_touches_it(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            self._init_repo_with_base_commit(repo)

            edited = self._OVER_LIMIT_FUNCTION.replace(
                "    if x == 9 { return 9; }",
                "    if x == 9 {\n        return 90;\n    }",
            )
            self._write_and_commit(
                repo, edited + "\n", "reformat the branch onto three lines and change its value"
            )

            with mock.patch("gate_diff.ROOT", repo):
                ranges = gate_diff.changed_ranges("HEAD~1")

            self.assertIn("src/lib.rs", ranges)
            self.assertTrue(
                gate_diff.any_intersect(ranges["src/lib.rs"], (1, 13)),
                "a real value change bundled with a reformat must not be normalized away, "
                f"got {ranges}",
            )


class FunctionsIn(unittest.TestCase):
    def _func_space(self, name: str | None, start: int, end: int, cyclomatic: int, children=()) -> dict:
        return {
            "kind": "function",
            "name": name,
            "start_line": start,
            "end_line": end,
            "metrics": {"cyclomatic": {"sum": cyclomatic}},
            "spaces": list(children),
        }

    def test_named_function_with_no_closures_is_reported_once(self) -> None:
        out: list[dict] = []
        complexity_gate.functions_in(self._func_space("f", 1, 10, 5), out)
        self.assertEqual(len(out), 1)
        self.assertEqual(out[0]["name"], "f")

    def test_closure_nested_in_a_named_function_is_not_reported_separately(self) -> None:
        # The exact bug: `main` containing a closure that is essentially its
        # whole body must be one violation, not two, when both are over the
        # limit -- the closure is not independently-invocable code, it *is*
        # `main`'s body.
        closure = self._func_space(None, 2, 9, 25)
        outer = self._func_space("main", 1, 10, 27, children=[closure])
        out: list[dict] = []
        complexity_gate.functions_in(outer, out)
        self.assertEqual(len(out), 1)
        self.assertEqual(out[0]["name"], "main")
        self.assertEqual(out[0]["cyclomatic"], 27)

    def test_closure_nested_in_a_closure_still_collapses_to_one_report(self) -> None:
        inner_closure = self._func_space(None, 3, 8, 10)
        outer_closure = self._func_space(None, 2, 9, 15, children=[inner_closure])
        outer = self._func_space("run", 1, 10, 20, children=[outer_closure])
        out: list[dict] = []
        complexity_gate.functions_in(outer, out)
        self.assertEqual(len(out), 1)
        self.assertEqual(out[0]["name"], "run")

    def test_named_function_nested_inside_a_closure_is_still_reported(self) -> None:
        # Legal, if unusual, Rust: a closure body can define its own nested
        # `fn`. That is independently real code -- unlike an anonymous
        # closure, it is not "the same bytes" as anything enclosing it --
        # so it must survive the collapse that swallows anonymous closures.
        nested_fn = self._func_space("helper", 3, 5, 8)
        closure = self._func_space(None, 2, 6, 9, children=[nested_fn])
        outer = self._func_space("run", 1, 7, 12, children=[closure])
        out: list[dict] = []
        complexity_gate.functions_in(outer, out)
        names = {f["name"] for f in out}
        self.assertEqual(names, {"run", "helper"})

    def test_top_level_closure_with_no_enclosing_function_is_still_reported(self) -> None:
        # A closure with nothing to fold into (e.g. a module-level `static`
        # initializer) is the only code there -- it must not be dropped.
        top_level_closure = self._func_space(None, 1, 5, 12)
        out: list[dict] = []
        complexity_gate.functions_in(top_level_closure, out)
        self.assertEqual(len(out), 1)
        self.assertEqual(out[0]["name"], "<anonymous>")

    def test_rust_code_analysis_cli_names_a_closure_the_literal_string_anonymous(self) -> None:
        # The real bug, found by running the real tool rather than trusting
        # a `None`/empty assumption: `rust-code-analysis-cli` puts the
        # literal string "<anonymous>" in a closure's own `name` field, it
        # does not leave it `None` or empty. A plain truthiness check on
        # `name` treats that literal string as "named" and never collapses
        # anything -- this pins the real value shape, not an assumed one.
        closure = self._func_space("<anonymous>", 2, 9, 25)
        outer = self._func_space("main", 1, 10, 27, children=[closure])
        out: list[dict] = []
        complexity_gate.functions_in(outer, out)
        self.assertEqual(len(out), 1)
        self.assertEqual(out[0]["name"], "main")

    def test_sibling_functions_are_both_reported(self) -> None:
        root = {
            "kind": "file",
            "spaces": [
                self._func_space("a", 1, 5, 5),
                self._func_space("b", 10, 15, 5),
            ],
        }
        out: list[dict] = []
        complexity_gate.functions_in(root, out)
        self.assertEqual({f["name"] for f in out}, {"a", "b"})


class ComplexityFindViolations(unittest.TestCase):
    def test_flags_only_functions_the_diff_touches(self) -> None:
        ranges = {"src/lib.rs": [(10, 15)]}
        new_functions = {
            "src/lib.rs": [
                {"name": "touched_and_complex", "start": 10, "end": 15, "cyclomatic": 25},
                {"name": "untouched_and_complex", "start": 100, "end": 120, "cyclomatic": 99},
                {"name": "touched_but_simple", "start": 12, "end": 13, "cyclomatic": 3},
            ]
        }
        violations = complexity_gate.find_violations(ranges, new_functions, {}, max_cyclomatic=20)
        self.assertEqual(len(violations), 1)
        self.assertIn("touched_and_complex", violations[0])
        self.assertIn("is new at 25", violations[0])

    def test_no_violations_when_nothing_over_the_limit(self) -> None:
        ranges = {"src/lib.rs": [(1, 100)]}
        new_functions = {"src/lib.rs": [{"name": "fine", "start": 1, "end": 10, "cyclomatic": 5}]}
        self.assertEqual(complexity_gate.find_violations(ranges, new_functions, {}, 20), [])

    def test_untouched_file_contributes_no_violations(self) -> None:
        ranges = {"src/other.rs": [(1, 5)]}
        new_functions = {
            "src/lib.rs": [{"name": "huge", "start": 1, "end": 500, "cyclomatic": 500}]
        }
        self.assertEqual(complexity_gate.find_violations(ranges, new_functions, {}, 20), [])

    def test_already_over_limit_function_untouched_by_a_real_edit_passes(self) -> None:
        # The load-bearing case in the other direction: `run` was 174 before
        # this diff and is still 174 after it (the diff only deleted a
        # comment elsewhere in its span) -- existing debt, not new, so it
        # must not trip the gate just because a nearby hunk is in scope.
        ranges = {"src/lib.rs": [(1, 300)]}
        new_functions = {"src/lib.rs": [{"name": "run", "start": 1, "end": 300, "cyclomatic": 174}]}
        old_functions = {"src/lib.rs": [{"name": "run", "start": 1, "end": 305, "cyclomatic": 174}]}
        self.assertEqual(
            complexity_gate.find_violations(ranges, new_functions, old_functions, 20), []
        )

    def test_already_over_limit_function_made_simpler_passes(self) -> None:
        ranges = {"src/lib.rs": [(1, 300)]}
        new_functions = {"src/lib.rs": [{"name": "run", "start": 1, "end": 300, "cyclomatic": 150}]}
        old_functions = {"src/lib.rs": [{"name": "run", "start": 1, "end": 305, "cyclomatic": 174}]}
        self.assertEqual(
            complexity_gate.find_violations(ranges, new_functions, old_functions, 20), []
        )

    def test_already_over_limit_function_made_worse_still_trips_the_gate(self) -> None:
        # The load-bearing test the whole before/after rule hinges on: this
        # must still fail, or the gate stops meaning anything. Raising an
        # already-bad function's complexity is exactly the case a reviewer
        # needs flagged -- "already over the limit" must never become a
        # license to make it worse for free.
        ranges = {"src/lib.rs": [(1, 300)]}
        new_functions = {"src/lib.rs": [{"name": "run", "start": 1, "end": 300, "cyclomatic": 180}]}
        old_functions = {"src/lib.rs": [{"name": "run", "start": 1, "end": 305, "cyclomatic": 174}]}
        violations = complexity_gate.find_violations(ranges, new_functions, old_functions, 20)
        self.assertEqual(len(violations), 1)
        self.assertIn("was 174, is now 180", violations[0])

    def test_under_the_limit_before_and_over_after_still_trips_the_gate(self) -> None:
        ranges = {"src/lib.rs": [(1, 20)]}
        new_functions = {"src/lib.rs": [{"name": "f", "start": 1, "end": 20, "cyclomatic": 25}]}
        old_functions = {"src/lib.rs": [{"name": "f", "start": 1, "end": 18, "cyclomatic": 18}]}
        violations = complexity_gate.find_violations(ranges, new_functions, old_functions, 20)
        self.assertEqual(len(violations), 1)
        self.assertIn("was 18, is now 25", violations[0])

    def test_new_function_with_no_old_counterpart_is_judged_against_the_limit(self) -> None:
        ranges = {"src/lib.rs": [(1, 20)]}
        new_functions = {"src/lib.rs": [{"name": "brand_new", "start": 1, "end": 20, "cyclomatic": 25}]}
        violations = complexity_gate.find_violations(ranges, new_functions, {}, 20)
        self.assertEqual(len(violations), 1)
        self.assertIn("is new at 25", violations[0])

    def test_same_named_functions_are_matched_by_occurrence_order(self) -> None:
        # Two unrelated structs each define `fn new()`. The first should be
        # compared against the first, the second against the second -- not
        # "last one wins," which would compare the first new-side occurrence
        # against the second old-side one.
        ranges = {"src/lib.rs": [(1, 5), (10, 15)]}
        new_functions = {
            "src/lib.rs": [
                {"name": "new", "start": 1, "end": 5, "cyclomatic": 22},
                {"name": "new", "start": 10, "end": 15, "cyclomatic": 30},
            ]
        }
        old_functions = {
            "src/lib.rs": [
                {"name": "new", "start": 1, "end": 5, "cyclomatic": 22},
                {"name": "new", "start": 9, "end": 14, "cyclomatic": 18},
            ]
        }
        violations = complexity_gate.find_violations(ranges, new_functions, old_functions, 20)
        # The first `new` (22 -> 22, unchanged) passes; the second
        # (18 -> 30, worse) fails. A "last one wins" name match would
        # instead compare 22 against 18 and 30 against 22, getting both
        # judgements wrong.
        self.assertEqual(len(violations), 1)
        self.assertIn(":10-15", violations[0])
        self.assertIn("was 18, is now 30", violations[0])

    def test_anonymous_function_has_no_old_counterpart_even_if_old_side_has_one(self) -> None:
        # A top-level anonymous closure (see `FunctionsIn`) has no stable
        # identity across a diff, so it is always judged against the limit
        # directly -- it must not accidentally match some unrelated
        # anonymous entry on the old side.
        ranges = {"src/lib.rs": [(1, 5)]}
        new_functions = {
            "src/lib.rs": [{"name": "<anonymous>", "start": 1, "end": 5, "cyclomatic": 25}]
        }
        old_functions = {
            "src/lib.rs": [{"name": "<anonymous>", "start": 1, "end": 5, "cyclomatic": 99}]
        }
        violations = complexity_gate.find_violations(ranges, new_functions, old_functions, 20)
        self.assertEqual(len(violations), 1)
        self.assertIn("is new at 25", violations[0])


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
        violations = duplication_gate.find_violations([dup], [], ranges)
        self.assertEqual(len(violations), 1)
        self.assertIn("src/new.rs:5-16 (new)", violations[0])
        self.assertNotIn("src/old.rs:100-111 (new)", violations[0])

    def test_two_untouched_clones_are_not_flagged(self) -> None:
        ranges = {"src/elsewhere.rs": [(1, 5)]}
        dup = self._dup(("src/old_a.rs", 1, 12), ("src/old_b.rs", 1, 12))
        self.assertEqual(duplication_gate.find_violations([dup], [], ranges), [])

    def test_new_code_duplicating_itself_flags_both_sides(self) -> None:
        ranges = {"src/new.rs": [(1, 50)]}
        dup = self._dup(("src/new.rs", 1, 12), ("src/new.rs", 20, 31))
        violations = duplication_gate.find_violations([dup], [], ranges)
        self.assertEqual(len(violations), 1)
        self.assertIn("(new)", violations[0])

    def test_touched_clone_already_duplicated_before_passes(self) -> None:
        # The load-bearing case: this same pair of files already duplicated
        # each other before the diff (a nearby comment-removal collapse is
        # what makes one side "touched," not a new clone) -- existing debt,
        # not introduced by this diff, so it must not trip the gate.
        ranges = {"src/a.rs": [(10, 12)]}
        new_dup = self._dup(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), lines=12)
        old_dup = self._dup(("src/a.rs", 9, 20), ("src/b.rs", 38, 49), lines=12)
        self.assertEqual(duplication_gate.find_violations([new_dup], [old_dup], ranges), [])

    def test_touched_clone_with_no_old_counterpart_still_fails(self) -> None:
        # The dual, equally load-bearing case: nothing comparable existed
        # between these two files before, so this diff genuinely introduced
        # the duplication and the gate must still catch it.
        ranges = {"src/a.rs": [(10, 12)]}
        new_dup = self._dup(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), lines=12)
        violations = duplication_gate.find_violations([new_dup], [], ranges)
        self.assertEqual(len(violations), 1)
        self.assertIn("introduced by this diff", violations[0])

    def test_unrelated_old_clone_between_the_same_files_grants_no_amnesty(self) -> None:
        # Pinned safety property: two files sharing *some* old duplication
        # relationship must not blanket-exempt every future clone between
        # them. A wildly different-sized old clone between the same pair is
        # treated as a different clone, not the same one reformatted.
        ranges = {"src/a.rs": [(10, 12)]}
        new_dup = self._dup(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), lines=12)
        unrelated_old_dup = self._dup(("src/a.rs", 200, 299), ("src/b.rs", 300, 399), lines=100)
        violations = duplication_gate.find_violations([new_dup], [unrelated_old_dup], ranges)
        self.assertEqual(len(violations), 1)

    def test_reformatted_clone_within_size_tolerance_still_matches(self) -> None:
        # A comment-removal-driven collapse can shrink a clone's line count
        # a little without changing what code it is -- the match must
        # tolerate that instead of demanding an exact line count.
        ranges = {"src/a.rs": [(10, 12)]}
        new_dup = self._dup(("src/a.rs", 10, 19), ("src/b.rs", 40, 49), lines=10)
        old_dup = self._dup(("src/a.rs", 9, 20), ("src/b.rs", 38, 49), lines=12)
        self.assertEqual(duplication_gate.find_violations([new_dup], [old_dup], ranges), [])


class FilePair(unittest.TestCase):
    def test_order_independent(self) -> None:
        a = {"firstFile": {"name": "x.rs"}, "secondFile": {"name": "y.rs"}}
        b = {"firstFile": {"name": "y.rs"}, "secondFile": {"name": "x.rs"}}
        self.assertEqual(duplication_gate.file_pair(a), duplication_gate.file_pair(b))


class AlreadyDuplicatedBefore(unittest.TestCase):
    def _dup(self, first: str, second: str, lines: int) -> dict:
        return {
            "firstFile": {"name": first, "start": 1, "end": 1},
            "secondFile": {"name": second, "start": 1, "end": 1},
            "lines": lines,
        }

    def test_no_old_duplicates_at_all(self) -> None:
        candidate = self._dup("a.rs", "b.rs", 12)
        self.assertFalse(duplication_gate.already_duplicated_before(candidate, []))

    def test_matching_pair_within_size_tolerance(self) -> None:
        candidate = self._dup("a.rs", "b.rs", 12)
        old = [self._dup("a.rs", "b.rs", 10)]
        self.assertTrue(duplication_gate.already_duplicated_before(candidate, old))

    def test_matching_pair_outside_size_tolerance_does_not_count(self) -> None:
        candidate = self._dup("a.rs", "b.rs", 12)
        old = [self._dup("a.rs", "b.rs", 100)]
        self.assertFalse(duplication_gate.already_duplicated_before(candidate, old))

    def test_different_pair_does_not_count(self) -> None:
        candidate = self._dup("a.rs", "b.rs", 12)
        old = [self._dup("a.rs", "c.rs", 12)]
        self.assertFalse(duplication_gate.already_duplicated_before(candidate, old))


if __name__ == "__main__":
    unittest.main()
