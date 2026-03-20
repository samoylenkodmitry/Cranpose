# Text Scroll Exact Contract

## Goal

Make the external scroll stability contract pass with zero pixel differences.

The exact target is:

- every exact diff image `diff_00_01.png` through `diff_08_09.png` is completely clean
- the compare script exits `0`
- `cargo run -p desktop-app --example robot_text_scroll_exact_external_contract --features robot-app` exits `0`

This is not "improve it a bit".
This is not "make the fractional diagnostic better".
This is not "explain why it is hard".
The goal is exact pass.

## Current Test Bench

- Robot example:
  `/home/s/develop/projects/compose-rs-proposal/apps/desktop-demo/robot-runners/robot_text_scroll_exact_external_contract.rs`
- Compare script:
  `/home/s/develop/projects/compose-rs-proposal/scripts/text_scroll_exact_external_compare.py`
- Output directory:
  `/home/s/develop/projects/compose-rs-proposal/docs/screenshots/text_scroll_exact_external`

Current artifacts:

- Exact lane:
  - `stabilized_text_scroll_step00.png` through `stabilized_text_scroll_step09.png`
  - `diff_00_01.png` through `diff_08_09.png`
- Fractional diagnostic lane:
  - `fractional_stabilized_text_scroll_step00.png` through `fractional_stabilized_text_scroll_step09.png`
  - `fractional_diff_00_01.png` through `fractional_diff_08_09.png`
  - `fractional_alignment_report.txt`

## Rules

- Obey `AGENTS.md` and repo instructions strictly.
- Do not weaken the contract.
- Do not add thresholds, fuzz, tolerances, or "good enough" logic.
- Do not switch the strict contract to internal screenshots.
- Do not use fractional-resampled images for pass/fail.
- Fractional alignment is diagnostic only.
- No unsafe.
- No half-state refactors.
- Fix the real cause, not symptoms.
- Prefer architectural cleanup over piling on branches and fallbacks.
- Keep the output directory semantics intact.

## Interpretation Rule

It does not matter whether the root cause is:

- scroll input not landing exactly where expected
- semantics position not matching presented pixels
- offscreen composition instability
- rerasterization instability
- caching instability
- any other renderer architecture issue

The job is to prove the cause with evidence, fix it, and make the exact contract pass.

## Execution Loop

1. Reproduce the failure with:
   `cargo run -p desktop-app --example robot_text_scroll_exact_external_contract --features robot-app`
2. Inspect:
   - exact diffs
   - stabilized exact sequence
   - fractional report
   - relevant renderer code paths
3. Rank the top root-cause hypotheses with concrete evidence from code and artifacts.
4. Implement the strongest fix.
5. Rerun the exact contract.
6. If still red, repeat immediately.
7. Do not stop on partial improvement.
8. Continue until the exact contract is green.

## Acceptance Criteria

- `cargo run -p desktop-app --example robot_text_scroll_exact_external_contract --features robot-app` returns success
- exact `diff_00_01.png` through `diff_08_09.png` contain no differences
- compare script returns `0`
- `./run_robot_test.sh --sequential` includes this robot and passes

## Full Validation After The Contract Turns Green

- `cargo fmt --all`
- `cargo test > 1.tmp 2>&1`, then read and fix all failures
- `cargo clippy > 2.tmp 2>&1`, then read and fix all warnings and failures
- `apps/desktop-demo/build-web.sh`
- in `/home/s/develop/projects/compose-rs-proposal/apps/android-demo/android`, run `./gradlew :app:assembleRelease`
- `./run_robot_test.sh --sequential`

## Working Style

- Give short progress updates while working.
- When blocked, show evidence, not guesses.
- When finished, report the real root cause, the architectural correction, and the commands that now pass.

Do not stop until the exact external scroll contract is green.
