// `LiquidToolbar` through the headless robot: each item reads as a button and
// runs its own action; the capsules sit 28 points in from the window's sides
// and bottom, 12 points apart; adjacent icons share one capsule and a text
// item between them splits it; flexible spaces share the free room.

use std::{cell::RefCell, rc::Rc};

use cranpose_liquid::prelude::*;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    Alignment, Modifier, SemanticsWidgetRole,
    widgets::{Box, BoxSpec},
};

use crate::reader::{Robot, node};

type Log = Rc<RefCell<Vec<&'static str>>>;

// A toolbar at the bottom of a 400 by 800 window, built by `items`, which
// records each tap in the log.
fn toolbar(items: fn(&LiquidToolbarScope, &Log)) -> (Robot, Log) {
    let log: Log = Rc::default();
    let record = Rc::clone(&log);
    let mut robot = create_headless_robot_test(400, 800, move || {
        let record = Rc::clone(&record);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let record = Rc::clone(&record);
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default().content_alignment(Alignment::BOTTOM_START),
                move || {
                    let record = Rc::clone(&record);
                    LiquidToolbar(Modifier::empty(), move |scope| items(scope, &record));
                },
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    (robot, log)
}

fn tap(log: &Log, name: &'static str) -> impl Fn() + 'static {
    let log = Rc::clone(log);
    move || log.borrow_mut().push(name)
}

fn edit_share_delete(scope: &LiquidToolbarScope, log: &Log) {
    scope.button("Edit", tap(log, "edit"));
    scope.flexible_space();
    scope.button("Share", tap(log, "share"));
    scope.prominent_button("Delete", tap(log, "delete"));
}

fn icons_text_icon(scope: &LiquidToolbarScope, log: &Log) {
    scope.icon_button(icons::SHARE, "Share", tap(log, "share"));
    scope.icon_button(icons::TRASH, "Delete", tap(log, "delete"));
    scope.button("Edit", tap(log, "edit"));
    scope.icon_button(icons::PLUS, "Add", tap(log, "add"));
    scope.flexible_space();
    scope.icon_button(icons::FOLDER, "Move", tap(log, "move"));
}

#[test]
fn each_item_reads_as_a_button_and_runs_its_own_action() {
    let (mut robot, log) = toolbar(icons_text_icon);
    for label in ["Share", "Delete", "Edit", "Add", "Move"] {
        let item = node(&mut robot, label);
        assert_eq!(
            item.widget_role,
            Some(SemanticsWidgetRole::Button),
            "{label}"
        );
        let bounds = item.layout_bounds;
        robot.click_at(
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        );
        robot.wait_for_idle();
    }
    assert_eq!(*log.borrow(), ["share", "delete", "edit", "add", "move"]);
}

#[test]
fn capsules_sit_28_points_in_and_12_apart() {
    let (mut robot, _) = toolbar(edit_share_delete);
    let edit = node(&mut robot, "Edit").layout_bounds;
    let share = node(&mut robot, "Share").layout_bounds;
    let delete = node(&mut robot, "Delete").layout_bounds;
    for bounds in [edit, share, delete] {
        assert!((bounds.height - 48.0).abs() < 0.01, "48 tall: {bounds:?}");
        assert!(
            (bounds.y + bounds.height - 772.0).abs() < 0.01,
            "28 above the bottom: {bounds:?}"
        );
    }
    assert!(
        (edit.x - 28.0).abs() < 0.01,
        "28 from the leading side: {edit:?}"
    );
    assert!(
        (delete.x + delete.width - 372.0).abs() < 0.01,
        "28 from the trailing side: {delete:?}"
    );
    assert!(
        (delete.x - share.x - share.width - 12.0).abs() < 0.01,
        "12 apart: {share:?} {delete:?}"
    );
}

#[test]
fn adjacent_icons_share_a_capsule_that_a_text_item_splits() {
    let (mut robot, _) = toolbar(icons_text_icon);
    let share = node(&mut robot, "Share").layout_bounds;
    let delete = node(&mut robot, "Delete").layout_bounds;
    let edit = node(&mut robot, "Edit").layout_bounds;
    let add = node(&mut robot, "Add").layout_bounds;
    let moved = node(&mut robot, "Move").layout_bounds;
    assert!((share.x - 28.0).abs() < 0.01, "{share:?}");
    assert!(
        (delete.x - share.x - share.width).abs() < 0.01,
        "the pair abuts: {share:?} {delete:?}"
    );
    assert!(
        (delete.x + delete.width - share.x - 105.0).abs() < 0.01,
        "the pair's capsule is 105 wide"
    );
    assert!(
        (edit.x - delete.x - delete.width - 12.0).abs() < 0.01,
        "the text item stands apart"
    );
    assert!(
        (add.x - edit.x - edit.width - 12.0).abs() < 0.01,
        "the icon after it starts a capsule"
    );
    for lone in [add, moved] {
        assert!(
            (lone.width - 48.0).abs() < 0.01,
            "a lone icon is a 48 point circle: {lone:?}"
        );
    }
    assert!((moved.x + moved.width - 372.0).abs() < 0.01, "{moved:?}");
}

#[test]
fn flexible_spaces_share_the_free_room() {
    fn centered(scope: &LiquidToolbarScope, log: &Log) {
        scope.flexible_space();
        scope.button("Edit", tap(log, "edit"));
        scope.flexible_space();
    }
    let (mut robot, _) = toolbar(centered);
    let edit = node(&mut robot, "Edit").layout_bounds;
    let before = edit.x - 28.0 - 12.0;
    let after = 372.0 - edit.x - edit.width - 12.0;
    // Each space takes whole pixels, so the shares may differ by one.
    assert!(
        before > 100.0 && (before - after).abs() <= 1.0,
        "equal shares: {edit:?}"
    );
}
