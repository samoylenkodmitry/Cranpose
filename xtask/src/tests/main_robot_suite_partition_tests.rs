use crate::robot_suite_partition::{find_violations, recipe_flag_values};

fn justfile(gpu_skips: &[&str], capture_examples: &[&str]) -> String {
    let mut text = String::from("# a comment\nrobot-gpu:\n    xvfb-run ./run_robot_test.sh \\\n");
    for name in gpu_skips {
        text.push_str(&format!("      --skip {name} \\\n"));
    }
    text.push_str("\nrobot-captures:\n    xvfb-run ./run_robot_test.sh \\\n");
    for name in capture_examples {
        text.push_str(&format!("      --example {name} \\\n"));
    }
    text.push_str("\nunrelated:\n    --skip not_in_either\n");
    text
}

#[test]
fn matching_halves_are_clean() {
    let text = justfile(&["alpha", "beta"], &["beta", "alpha"]);
    assert!(
        find_violations(&text).is_empty(),
        "{:?}",
        find_violations(&text)
    );
}

#[test]
fn an_example_skipped_but_never_captured_runs_nowhere() {
    let text = justfile(&["alpha", "beta"], &["alpha"]);
    let violations = find_violations(&text);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("beta") && violations[0].contains("nowhere"),
        "{violations:?}"
    );
}

#[test]
fn an_example_captured_but_not_skipped_runs_twice() {
    let text = justfile(&["alpha"], &["alpha", "beta"]);
    let violations = find_violations(&text);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("beta") && violations[0].contains("twice"),
        "{violations:?}"
    );
}

#[test]
fn an_empty_half_fails_rather_than_passing_vacuously() {
    // Two empty sets are equal. Without the emptiness check this gate
    // would pass on a justfile it could not parse.
    let text = justfile(&[], &[]);
    let violations = find_violations(&text);
    assert_eq!(
        violations.len(),
        2,
        "both halves must be reported: {violations:?}"
    );
}

#[test]
fn a_recipe_body_ends_at_the_next_column_zero_line() {
    // `unrelated` also contains `--skip`; it must not leak into robot-gpu.
    let text = justfile(&["alpha"], &["alpha"]);
    let skipped = recipe_flag_values(&text, "robot-gpu", "--skip");
    assert!(
        !skipped.contains("not_in_either"),
        "leaked from a later recipe: {skipped:?}"
    );
    assert_eq!(skipped.len(), 1);
}

#[test]
fn a_parameterised_recipe_header_still_names_its_recipe() {
    let text = concat!(
        "robot-gpu classes=\"all\":\n",
        "    ./run_robot_test.sh --classes {{classes}} \\\n",
        "      --skip robot_underline_screenshot\n",
        "\n",
        "robot-gpu-fast: (robot-gpu \"parallel\")\n",
    );
    let skipped = recipe_flag_values(text, "robot-gpu", "--skip");
    assert_eq!(
        skipped.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["robot_underline_screenshot"],
        "a recipe with parameters declares the same recipe"
    );
    assert!(
        recipe_flag_values(text, "robot-gpu-fast", "--skip").is_empty(),
        "a recipe whose name merely starts with another's is a different recipe"
    );
}

#[test]
fn the_real_justfile_halves_agree_and_are_not_empty() {
    let root = crate::workspace_root().expect("workspace root");
    let text = std::fs::read_to_string(root.join("justfile")).expect("justfile is readable");
    let skipped = recipe_flag_values(&text, "robot-gpu", "--skip");
    assert!(!skipped.is_empty(), "the gate must find the real skip list");
    assert!(
        find_violations(&text).is_empty(),
        "{:?}",
        find_violations(&text)
    );
}
