use cranpose_core::rememberMutableStateOf;
use cranpose_liquid::prelude::*;
use cranpose_testing::{
    RobotTestRule, TestRenderer, create_headless_robot_test, placed_semantics_from_shell,
};
use cranpose_ui::Modifier;

#[test]
fn a_navigation_header_announces_its_title_once_across_collapse() {
    for offset in [0.0, 26.0, 52.0] {
        let mut robot = create_headless_robot_test(400, 300, move || {
            LiquidTheme(LiquidThemeSpec::default(), move || {
                let scroll = cranpose_ui::rememberScrollState!(offset);
                LiquidNavBar(
                    Modifier::empty().fill_max_width(),
                    LiquidNavBarSpec::new("Library"),
                    scroll,
                    || {},
                    || {},
                );
            });
        });
        robot.shell_mut().set_semantics_enabled(true);
        robot.wait_for_idle();
        let tree =
            placed_semantics_from_shell(robot.shell_mut()).expect("placed navigation header");
        assert_eq!(
            tree.flatten()
                .into_iter()
                .filter(|node| !node.hidden && node.label.as_deref() == Some("Library"))
                .count(),
            1
        );
    }
}

#[test]
fn search_fields_keep_a_native_height_and_an_accessible_placeholder() {
    for on_glass in [false, true] {
        let mut robot = create_headless_robot_test(400, 300, move || {
            LiquidTheme(LiquidThemeSpec::default(), move || {
                let state =
                    cranpose_core::remember(|| cranpose_foundation::text::TextFieldState::new(""))
                        .with(|state| *state);
                LiquidSearchField(
                    Modifier::empty().width(300.0),
                    state,
                    LiquidSearchFieldSpec {
                        on_glass,
                        ..Default::default()
                    },
                );
            });
        });
        robot.shell_mut().set_semantics_enabled(true);
        robot.wait_for_idle();
        let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed search field");
        let node = tree
            .flatten()
            .into_iter()
            .find(|node| node.label.as_deref() == Some("Search"))
            .expect("the placeholder announces the search field");
        assert_eq!(node.layout_bounds.width, 300.0);
        assert_eq!(node.layout_bounds.height, 44.0);
    }
}

#[test]
fn text_button_sizes_preserve_accessible_activation_and_increase_the_touch_target() {
    let mut previous_height = 0.0;
    for size in [
        GlassButtonSize::Small,
        GlassButtonSize::Regular,
        GlassButtonSize::Large,
    ] {
        let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
        let recorded = std::rc::Rc::clone(&clicks);
        let mut robot = create_headless_robot_test(400, 300, move || {
            let recorded = std::rc::Rc::clone(&recorded);
            LiquidTheme(LiquidThemeSpec::default(), move || {
                let spec = GlassButtonSpec::glass().with_size(size);
                GlassButton(
                    Modifier::empty(),
                    spec.clone(),
                    move || recorded.set(recorded.get() + 1),
                    move || GlassButtonLabel("Continue", spec.clone()),
                );
            });
        });
        robot.shell_mut().set_semantics_enabled(true);
        robot.wait_for_idle();
        let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed button");
        let node = tree
            .flatten()
            .into_iter()
            .find(|node| node.label.as_deref() == Some("Continue") && node.clickable)
            .expect("announced action");
        assert!(node.layout_bounds.height > previous_height);
        previous_height = node.layout_bounds.height;
        activate(&mut robot, "Continue");
        assert_eq!(clicks.get(), 1);
    }
}

#[test]
fn reader_activates_a_glass_button_beyond_the_scroll_viewport() {
    let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
    let recorded = std::rc::Rc::clone(&clicks);
    let mut robot = create_headless_robot_test(400, 300, move || {
        let recorded = std::rc::Rc::clone(&recorded);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let recorded = std::rc::Rc::clone(&recorded);
            let scroll = cranpose_ui::rememberScrollState!(0.0);
            cranpose_ui::Column(
                Modifier::empty()
                    .fill_max_size()
                    .vertical_scroll(scroll, false),
                cranpose_ui::ColumnSpec::default(),
                move || {
                    cranpose_ui::Spacer(Modifier::empty().size_points(1.0, 600.0));
                    let recorded = std::rc::Rc::clone(&recorded);
                    GlassButton(
                        Modifier::empty().fill_max_width(),
                        GlassButtonSpec::glass(),
                        move || recorded.set(recorded.get() + 1),
                        || GlassButtonLabel("Edit more fields", GlassButtonSpec::glass()),
                    );
                },
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    activate(&mut robot, "Edit more fields");
    assert_eq!(clicks.get(), 1);
}

#[test]
fn reader_selects_each_tab_without_pointer_input() {
    for accessory in [false, true] {
        let mut robot = create_headless_robot_test(400, 800, move || {
            LiquidTheme(LiquidThemeSpec::default(), move || {
                let selected = rememberMutableStateOf(|| 0usize);
                let tabs = |tabs: &LiquidTabBarScope| {
                    tabs.tab(cranpose_liquid::icons::BOOKMARK, "Library");
                    tabs.tab(cranpose_liquid::icons::SEARCH, "Scan");
                    tabs.tab(cranpose_liquid::icons::STAR, "Settings");
                };
                if accessory {
                    LiquidTabBarWithAccessory(
                        Modifier::empty(),
                        LiquidTabBarSpec::default(),
                        selected.get(),
                        move |index| selected.set(index),
                        tabs,
                        || LiquidTabBarSearchAccessory(|| {}),
                    );
                } else {
                    LiquidTabBar(
                        Modifier::empty(),
                        LiquidTabBarSpec::default(),
                        selected.get(),
                        move |index| selected.set(index),
                        tabs,
                    );
                }
            });
        });
        robot.shell_mut().set_semantics_enabled(true);
        robot.wait_for_idle();
        for label in ["Settings", "Scan", "Library"] {
            activate(&mut robot, label);
            assert_selected(&mut robot, label);
        }
    }
}

fn activate(robot: &mut RobotTestRule<TestRenderer>, label: &str) {
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed controls");
    let node = tree
        .flatten()
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .expect("named control");
    assert!(node.clickable, "{label} advertises activation");
    assert!(
        robot.shell_mut().accessibility_activate(node.node_id, None),
        "{label}"
    );
    robot.wait_for_idle();
}

fn assert_selected(robot: &mut RobotTestRule<TestRenderer>, label: &str) {
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("updated controls");
    let selected: Vec<_> = tree
        .flatten()
        .into_iter()
        .filter(|node| node.selected == Some(true))
        .filter_map(|node| node.label.as_deref())
        .collect();
    assert_eq!(selected, [label]);
}

#[test]
fn keyboard_moves_across_tabs_and_selects_the_focused_tab() {
    for accessory in [false, true] {
        let mut robot = create_headless_robot_test(400, 800, move || {
            LiquidTheme(LiquidThemeSpec::default(), move || {
                let selected = rememberMutableStateOf(|| 0usize);
                let search =
                    cranpose_core::remember(|| cranpose_foundation::text::TextFieldState::new(""))
                        .with(|field| *field);
                cranpose_ui::BasicTextField(
                    search,
                    Modifier::empty()
                        .size(cranpose_ui::Size::new(200.0, 48.0))
                        .content_description("Search receipts"),
                    cranpose_ui::TextStyle::default(),
                );
                let tabs = |tabs: &LiquidTabBarScope| {
                    tabs.tab(cranpose_liquid::icons::BOOKMARK, "Library");
                    tabs.tab(cranpose_liquid::icons::SEARCH, "Scan");
                    tabs.tab(cranpose_liquid::icons::STAR, "Settings");
                };
                if accessory {
                    LiquidTabBarWithAccessory(
                        Modifier::empty(),
                        LiquidTabBarSpec::default(),
                        selected.get(),
                        move |index| selected.set(index),
                        tabs,
                        || LiquidTabBarSearchAccessory(|| {}),
                    );
                } else {
                    LiquidTabBar(
                        Modifier::empty(),
                        LiquidTabBarSpec::default(),
                        selected.get(),
                        move |index| selected.set(index),
                        tabs,
                    );
                }
            });
        });
        robot.shell_mut().set_semantics_enabled(true);
        robot.wait_for_idle();
        press_key(&mut robot, cranpose_ui::KeyCode::Tab, "\t");
        assert_focused(&mut robot, "Search receipts");
        press_key(&mut robot, cranpose_ui::KeyCode::Tab, "\t");
        assert_focused(&mut robot, "Library");
        for (key, focused) in [
            (cranpose_ui::KeyCode::ArrowRight, "Scan"),
            (cranpose_ui::KeyCode::End, "Settings"),
            (cranpose_ui::KeyCode::ArrowRight, "Library"),
            (cranpose_ui::KeyCode::ArrowLeft, "Settings"),
        ] {
            press_key(&mut robot, key, "");
            assert_focused(&mut robot, focused);
            assert_selected(&mut robot, "Library");
        }
        press_key(&mut robot, cranpose_ui::KeyCode::Enter, "\r");
        assert_selected(&mut robot, "Settings");
        assert_focused(&mut robot, "Settings");
    }
}

#[test]
fn reader_selects_each_segment_and_receives_the_committed_value() {
    let mut robot = create_headless_robot_test(400, 800, || {
        LiquidTheme(LiquidThemeSpec::default(), || {
            let selected = rememberMutableStateOf(|| 0usize);
            LiquidSegmentedControl(
                Modifier::empty().width(300.0),
                selected.get(),
                move |index| selected.set(index),
                |scope| {
                    for label in ["Day", "Month", "Year"] {
                        scope.segment(label);
                    }
                },
            );
            cranpose_ui::Text(
                "After segments",
                Modifier::empty().clickable(|_| {}),
                cranpose_ui::TextStyle::default(),
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    for label in ["Year", "Month", "Day"] {
        activate(&mut robot, label);
        assert_selected(&mut robot, label);
    }
    press_key(&mut robot, cranpose_ui::KeyCode::Tab, "\t");
    assert_focused(&mut robot, "Day");
    for (key, label) in [
        (cranpose_ui::KeyCode::ArrowRight, "Month"),
        (cranpose_ui::KeyCode::End, "Year"),
        (cranpose_ui::KeyCode::Home, "Day"),
    ] {
        press_key(&mut robot, key, "");
        assert_selected(&mut robot, label);
    }
    press_key(&mut robot, cranpose_ui::KeyCode::Tab, "\t");
    assert_focused(&mut robot, "After segments");
}

fn press_key(robot: &mut RobotTestRule<TestRenderer>, key: cranpose_ui::KeyCode, text: &str) {
    assert!(
        robot
            .shell_mut()
            .on_key_event(&cranpose_ui::KeyEvent::key_down(key, text))
    );
    robot.wait_for_idle();
}

fn assert_focused(robot: &mut RobotTestRule<TestRenderer>, label: &str) {
    let focused_id = robot
        .shell_mut()
        .app_context()
        .enter(cranpose_ui::active_focus_target);
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("focus semantics");
    let focused = tree
        .flatten()
        .into_iter()
        .find(|node| Some(node.node_id) == focused_id)
        .expect("focused control");
    assert_eq!(focused.label.as_deref(), Some(label));
}

#[test]
fn an_action_chip_is_a_plain_button_whatever_its_look() {
    let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
    let recorded = std::rc::Rc::clone(&clicks);
    let mut robot = create_headless_robot_test(400, 300, move || {
        let recorded = std::rc::Rc::clone(&recorded);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let save = std::rc::Rc::clone(&recorded);
            LiquidActionChip(
                Modifier::empty(),
                true,
                move || save.set(save.get() + 1),
                "Save",
            );
            LiquidActionChip(Modifier::empty(), false, || {}, "Cancel");
            LiquidChip(Modifier::empty(), false, || {}, "Receipts");
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed chips");
    let nodes = tree.flatten();
    let state = |label: &str| {
        nodes
            .iter()
            .find(|node| node.label.as_deref() == Some(label))
            .map(|node| node.selected)
            .expect("named chip")
    };
    assert_eq!(state("Save"), None);
    assert_eq!(state("Cancel"), None);
    assert_eq!(state("Receipts"), Some(false));
    activate(&mut robot, "Save");
    assert_eq!(clicks.get(), 1);
}

#[test]
fn reader_changes_a_switch_in_both_directions() {
    let mut robot = create_headless_robot_test(400, 800, || {
        LiquidTheme(LiquidThemeSpec::default(), || {
            let checked = rememberMutableStateOf(|| false);
            LiquidToggle(
                Modifier::empty().content_description("Use cellular data"),
                checked.get(),
                move |next| checked.set(next),
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    for expected in [true, false, true] {
        activate(&mut robot, "Use cellular data");
        let tree = placed_semantics_from_shell(robot.shell_mut()).expect("switch state");
        let switch = tree
            .flatten()
            .into_iter()
            .find(|node| node.label.as_deref() == Some("Use cellular data"))
            .expect("named switch");
        assert_eq!(switch.toggled, Some(expected));
    }
}

fn more_menu(
    modifier: Modifier,
    expanded: cranpose_core::MutableState<bool>,
    content: impl Fn(&LiquidMenuScope) + 'static,
) {
    LiquidDropdownMenu(
        modifier,
        expanded.get(),
        LiquidDropdownMenuSpec::default(),
        move || expanded.set(false),
        move || {
            cranpose_ui::Text(
                "More",
                Modifier::empty()
                    .size(cranpose_ui::Size::new(100.0, 48.0))
                    .clickable(move |_| expanded.set(true)),
                cranpose_ui::TextStyle::default(),
            );
        },
        content,
    );
}

#[test]
fn an_open_menu_names_itself_to_a_screen_reader() {
    let mut robot = create_headless_robot_test(400, 800, move || {
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let expanded = rememberMutableStateOf(|| false);
            more_menu(Modifier::empty().pane_title("Receipt"), expanded, |scope| {
                scope.item(LiquidMenuItem::new("Export text"), || {});
                scope.item(LiquidMenuItem::new("Copy text"), || {});
            });
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    activate(&mut robot, "More");

    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("open menu");
    let issues = cranpose_testing::audit_accessibility(&tree);
    assert!(
        !issues
            .iter()
            .any(|issue| issue.kind == cranpose_testing::AccessibilityIssueKind::NoPaneTitle),
        "an open menu hides the screen behind it, so it must name itself: {issues:?}"
    );
    assert!(
        tree.flatten()
            .into_iter()
            .any(|node| !node.hidden && node.pane_title.as_deref() == Some("Menu"))
    );
}

#[test]
fn menus_stay_centered_and_put_the_first_action_nearest_the_anchor() {
    for (anchor_x, anchor_y, grouped, center_x) in [
        (159.0, 100.0, false, 201.0),
        (159.0, 434.0, false, 201.0),
        (159.0, 434.0, true, 201.0),
        (159.0, 100.0, true, 201.0),
        (-20.0, 434.0, false, 141.0),
        (390.0, 434.0, false, 261.0),
    ] {
        let mut robot = create_headless_robot_test(402, 874, move || {
            LiquidTheme(LiquidThemeSpec::default(), move || {
                LiquidMenu(
                    true,
                    cranpose_ui_graphics::Rect {
                        x: anchor_x,
                        y: anchor_y,
                        width: 84.0,
                        height: 34.0,
                    },
                    LiquidMenuSpec::default(),
                    Vec::new(),
                    rememberLiquidMenuGesture(),
                    || {},
                    |scope| {
                        let labels: &[&str] = if grouped {
                            &["Document", "Copy", "Share", "Danger", "Delete", "Cancel"]
                        } else {
                            &["Copy", "Share", "Delete"]
                        };
                        for label in labels {
                            if matches!(*label, "Document" | "Danger") {
                                scope.header(*label);
                            } else {
                                scope.item(LiquidMenuItem::new(*label), || {});
                            }
                        }
                    },
                );
            });
        });
        robot.shell_mut().set_semantics_enabled(true);
        robot.wait_for_idle();
        let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed menu");
        let bounds = |label| {
            tree.flatten()
                .into_iter()
                .find(|node| node.label.as_deref() == Some(label))
                .expect("menu action")
                .layout_bounds
        };
        let copy = bounds("Copy");
        let delete = bounds("Delete");
        if grouped {
            assert!(bounds("Document").y < bounds("Share").y);
            assert!(bounds("Danger").y < bounds("Cancel").y);
        }
        assert!(
            (copy.x + copy.width * 0.5 - center_x).abs() <= 0.5,
            "menu must stay centered: {copy:?}"
        );
        assert!(
            (copy.height - 42.0).abs() <= 0.5,
            "native row height: {copy:?}"
        );
        if anchor_y > 400.0 {
            assert!(copy.y > delete.y, "upward menu starts beside the anchor");
            assert!(copy.y + copy.height <= anchor_y + 34.0);
        } else {
            assert!(copy.y < delete.y, "downward menu keeps declaration order");
            assert!(copy.y >= anchor_y);
        }
    }
}

#[test]
fn reader_selects_a_menu_item_and_dismisses_the_popup() {
    for keyboard in [false, true] {
        let result = std::rc::Rc::new(std::cell::Cell::new(0));
        let recorded = std::rc::Rc::clone(&result);
        let mut robot = create_headless_robot_test(400, 800, move || {
            let recorded = std::rc::Rc::clone(&recorded);
            LiquidTheme(LiquidThemeSpec::default(), move || {
                let expanded = rememberMutableStateOf(|| false);
                let recorded = std::rc::Rc::clone(&recorded);
                more_menu(Modifier::empty(), expanded, move |scope| {
                    scope.header("Document actions");
                    let copied = std::rc::Rc::clone(&recorded);
                    let recorded = std::rc::Rc::clone(&recorded);
                    scope.item(LiquidMenuItem::new("Export text"), move || {
                        recorded.set(recorded.get() + 1);
                    });
                    scope.item(LiquidMenuItem::new("Copy text"), move || {
                        copied.set(copied.get() + 10);
                    });
                });
            });
        });
        robot.shell_mut().set_semantics_enabled(true);
        robot.wait_for_idle();
        activate(&mut robot, "More");
        if keyboard {
            assert!(
                robot
                    .shell_mut()
                    .on_key_event(&cranpose_ui::KeyEvent::key_down(
                        cranpose_ui::KeyCode::ArrowDown,
                        ""
                    ))
            );
            assert!(
                robot
                    .shell_mut()
                    .on_key_event(&cranpose_ui::KeyEvent::key_down(
                        cranpose_ui::KeyCode::Enter,
                        "\r"
                    ))
            );
            robot.wait_for_idle();
        } else {
            activate(&mut robot, "Export text");
        }
        assert_eq!(result.get(), if keyboard { 10 } else { 1 });
        let tree = placed_semantics_from_shell(robot.shell_mut()).expect("closed menu");
        assert!(
            !tree
                .flatten()
                .into_iter()
                .any(|node| node.label.as_deref() == Some("Export text") && !node.hidden)
        );
        assert_focused(&mut robot, "More");
    }
}
