pub mod tab_switch_regression_support;

use cranpose_testing::{
    audit_accessibility, placed_semantics_from_shell,
    robot::{RobotTestRule, TestRenderer},
    AccessibilityIssue,
};
use desktop_app::app::{combined_app, DEMO_TAB_INFO};
use tab_switch_regression_support::{
    pump_shell_until_stable, set_active_tab, wait_for_active_tab_registration_robot,
};

/// The issues a tab still has, each with the reason it stays. A new issue
/// on any tab fails the test, and so does a listed one that went away: the
/// list only shrinks. The slug `*` stands for every tab.
const KNOWN: &[(&str, &str, &str)] = &[
    (
        "*",
        "SmallTarget: control \"Show source\" is 79x17 points",
        "the floating pill sits over the first tab; a larger target would cover it",
    ),
    (
        "liquid-ui",
        "SameName: tab \"Apps\" appears 2 times",
        "two tab bars show one set of tabs on purpose",
    ),
    (
        "liquid-ui",
        "SameName: tab \"Arcade\" appears 2 times",
        "two tab bars show one set of tabs on purpose",
    ),
    (
        "liquid-ui",
        "SameName: tab \"Games\" appears 2 times",
        "two tab bars show one set of tabs on purpose",
    ),
    (
        "liquid-ui",
        "SameName: tab \"Search\" appears 2 times",
        "two tab bars show one set of tabs on purpose",
    ),
    (
        "liquid-ui",
        "SameName: tab \"Today\" appears 2 times",
        "two tab bars show one set of tabs on purpose",
    ),
];

fn issues_per_tab() -> Vec<(&'static str, Vec<AccessibilityIssue>)> {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), combined_app);
    robot.shell_mut().set_semantics_enabled(true);
    wait_for_active_tab_registration_robot(&mut robot);
    let mut report = Vec::new();
    for info in &DEMO_TAB_INFO {
        set_active_tab(info.tab);
        robot.wait_for_idle();
        pump_shell_until_stable(robot.shell_mut());
        let placed = placed_semantics_from_shell(robot.shell_mut())
            .unwrap_or_else(|| panic!("the {} tab has no semantics tree", info.slug));
        report.push((info.slug, audit_accessibility(&placed)));
    }
    report
}

#[test]
fn every_demo_tab_reads_well_to_a_screen_reader() {
    let mut found: Vec<(String, String)> = issues_per_tab()
        .into_iter()
        .flat_map(|(slug, issues)| {
            issues
                .into_iter()
                .map(move |issue| (slug.to_string(), issue.to_string()))
        })
        .collect();
    found.sort();
    found.dedup();

    let is_known = |(slug, issue): &(String, String)| {
        KNOWN.iter().any(|(known_slug, known_issue, _)| {
            (*known_slug == "*" || *known_slug == slug) && *known_issue == issue
        })
    };
    let new: Vec<String> = found
        .iter()
        .filter(|entry| !is_known(entry))
        .map(|(slug, issue)| format!("{slug}: {issue}"))
        .collect();
    let gone: Vec<String> = KNOWN
        .iter()
        .filter(|(slug, issue, _)| {
            !found.iter().any(|(found_slug, found_issue)| {
                (*slug == "*" || *slug == found_slug) && *issue == found_issue
            })
        })
        .map(|(slug, issue, _)| format!("{slug}: {issue}"))
        .collect();
    assert!(
        new.is_empty(),
        "\nnew accessibility issues on the demo tabs:\n{}\n",
        new.join("\n")
    );
    assert!(
        gone.is_empty(),
        "\nlisted issues that are fixed; take them out of KNOWN:\n{}\n",
        gone.join("\n")
    );
}
