use std::fs;
use std::path::PathBuf;

const CHECKED_FILES: &[&str] = &[
    "src/app.rs",
    "src/app/animations.rs",
    "src/app/hacker_news.rs",
    "src/app/images.rs",
    "src/app/lazy_list.rs",
    "src/app/markdown.rs",
    "src/app/mineswapper2.rs",
    "src/app/shaders.rs",
    "src/app/web_fetch.rs",
    "src/tests/main_tests.rs",
    "robot-runners/robot_fling_precise.rs",
    "robot-runners/robot_lazy_lifecycle.rs",
    "robot-runners/robot_lazy_list.rs",
    "robot-runners/robot_lazy_varheight_lifecycle.rs",
    "robot-runners/robot_markdown_scrollbar.rs",
    "robot-runners/robot_measure_shaders.rs",
    "robot-runners/robot_perf_harness.rs",
    "robot-runners/robot_progress_bar.rs",
    "robot-runners/robot_shader_backdrop_drag.rs",
    "robot-runners/robot_shadow_fields.rs",
    "robot-runners/robot_subcompose_invalidation.rs",
    "robot-runners/robot_subcompose_lazy.rs",
    "robot-runners/robot_tab_selection.rs",
    "robot-runners/robot_text_showcase_gradient.rs",
    "robot-runners/robot_ui_breakage.rs",
];

#[test]
fn demo_sources_do_not_introduce_identifier_alias_boilerplate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();

    for relative_path in CHECKED_FILES {
        let path = root.join(relative_path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));

        for (line_number, line) in source.lines().enumerate() {
            if let Some((lhs, rhs)) = bare_identifier_alias(line) {
                offenders.push(format!(
                    "{}:{}: let {} = {};",
                    relative_path,
                    line_number + 1,
                    lhs,
                    rhs
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "found bare identifier alias boilerplate:\n{}",
        offenders.join("\n")
    );
}

fn bare_identifier_alias(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.starts_with("//") || !trimmed.starts_with("let ") || !trimmed.ends_with(';') {
        return None;
    }

    let body = trimmed.strip_prefix("let ")?.strip_suffix(';')?.trim();
    if body.contains(':') {
        return None;
    }

    let (lhs, rhs) = body.split_once('=')?;
    let lhs = lhs.trim().strip_prefix("mut ").unwrap_or(lhs.trim());
    let rhs = rhs.trim();
    if lhs.starts_with('_') || rhs.starts_with('_') {
        return None;
    }

    is_local_identifier(lhs).then_some(())?;
    is_local_identifier(rhs).then_some(())?;
    if matches!(rhs, "true" | "false") {
        return None;
    }
    Some((lhs, rhs))
}

fn is_local_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}
