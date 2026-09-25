use cranpose_ui::widgets::dialog::{Dialog, DialogSpec};

use super::*;

thread_local! {
    static READER_LENS_EVENTS: RefCell<Option<cranpose_core::EventSender<usize>>> = const { RefCell::new(None) };
}

#[test]
fn controls_inserted_by_a_recomposed_loop_have_unique_semantic_nodes() {
    let _guard = test_guard();
    let captured = Rc::new(RefCell::new(None));
    let content_state = Rc::clone(&captured);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let count = rememberMutableStateOf(|| 0usize);
            *content_state.borrow_mut() = Some(count);
            let route = usize::from(count.get() > 0);
            cranpose_ui::widgets::Scaffold(
                Modifier::empty().fill_max_size(),
                || {},
                || {},
                move |_| {
                    let content_size = rememberMutableStateOf(Size::default);
                    Box(
                        Modifier::empty()
                            .fill_max_size()
                            .report_size_state(content_size),
                        BoxSpec::default(),
                        move || {
                            if content_size.get().width <= 0.0 {
                                return;
                            }
                            Box(
                                Modifier::empty().fill_max_size(),
                                BoxSpec::default(),
                                move || {
                                    reader_screen_body(count, route);
                                },
                            );
                        },
                    );
                },
            );
        },
    );
    shell.set_semantics_enabled(true);
    let count = captured.borrow().expect("choice count");
    for expected in [0usize, 1, 2, 3, 4, 2, 0, 3] {
        READER_LENS_EVENTS.with(|events| {
            if let Some(sender) = events.borrow().as_ref() {
                sender.send(expected);
            }
        });
        for _ in 0..5 {
            shell.update();
        }
        count.set(expected);
        for _ in 0..5 {
            shell.update();
        }
        let root = shell.semantics_tree().expect("semantics").root();
        let mut ids = std::collections::BTreeSet::new();
        let mut labels = Vec::new();
        collect_unique_semantic_nodes(root, &mut ids, &mut labels);
        assert_eq!(labels.len(), if expected < 2 { 0 } else { expected });
        let layout = shell.layout_tree().expect("layout");
        for id in labels {
            let bounds = reader_node_bounds(layout.root(), id).expect("lens bounds");
            assert!(
                bounds.y >= 120.0,
                "lens {id} has incorrect bounds {bounds:?}"
            );
        }
    }
    READER_LENS_EVENTS.with(|events| events.borrow_mut().take());
}

fn collect_unique_semantic_nodes(
    node: &cranpose_ui::SemanticsNode,
    ids: &mut std::collections::BTreeSet<NodeId>,
    labels: &mut Vec<NodeId>,
) {
    assert!(
        ids.insert(node.node_id),
        "duplicate semantic node {}",
        node.node_id
    );
    if node
        .description
        .as_ref()
        .is_some_and(|label| label.starts_with("Lens ") && !node.actions.is_empty())
    {
        labels.push(node.node_id);
    }
    for descendant in &node.children {
        collect_unique_semantic_nodes(descendant, ids, labels);
    }
}

fn reader_node_bounds(node: &cranpose_ui::LayoutBox, id: NodeId) -> Option<Rect> {
    if node.node_id == id {
        return Some(node.rect);
    }
    node.children
        .iter()
        .find_map(|node| reader_node_bounds(node, id))
}

#[cranpose_ui::composable]
fn reader_screen_body(count: MutableState<usize>, route: usize) {
    match route {
        0 => {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                for index in 0..40 {
                    Box(Modifier::empty(), BoxSpec::default(), move || {
                        Text(
                            format!("Document {index}"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    });
                }
            });
        }
        _ => reader_camera_screen(count),
    }
}

#[cranpose_ui::composable]
fn reader_camera_screen(count: MutableState<usize>) {
    let title = format!("Camera {}", count.get());
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            let title = title.clone();
            Column(
                Modifier::empty().padding_each(0.0, 120.0, 0.0, 0.0),
                ColumnSpec::default(),
                move || {
                    Text(title.clone(), Modifier::empty(), TextStyle::default());
                    reader_lens_choices(count);
                },
            );
        },
    );
}

#[cranpose_ui::composable]
fn reader_lens_choices(count: MutableState<usize>) {
    let initial = count.get_non_reactive();
    let updates = cranpose_core::rememberEventStream((), move |sender| {
        sender.send(initial);
        READER_LENS_EVENTS.with(|events| *events.borrow_mut() = Some(sender));
    });
    let count = cranpose_core::collectAsState(updates, (), initial).get();
    let choices: Vec<_> = (0..count)
        .map(|index| (index.to_string(), format!("Lens {index}")))
        .collect();
    if choices.len() < 2 {
        return;
    }
    let active = (count - 1).to_string();
    Row(
        Modifier::empty().padding_each(6.0, 4.0, 6.0, 4.0),
        RowSpec::default(),
        move || {
            for (id, label) in choices.clone() {
                let picked = id == active;
                let told = label.clone();
                let mut pill = Modifier::empty()
                    .rounded_corners(999.0)
                    .padding_each(10.0, 5.0, 10.0, 5.0)
                    .semantics(move |config| config.content_description = Some(told.clone()));
                if picked {
                    pill = pill.background(Color(1.0, 1.0, 1.0, 0.14));
                }
                Box(
                    pill.clickable(move |_| {
                        let _ = (&id, picked);
                    }),
                    BoxSpec::default(),
                    move || {
                        Text(label.clone(), Modifier::empty(), TextStyle::default());
                    },
                );
            }
        },
    );
}

#[test]
fn nested_dialogs_focus_their_controls_and_restore_each_opener() {
    let _guard = test_guard();
    for reader in [false, true] {
        let mut shell = AppShell::new(
            HitGraphRenderer::default(),
            location_key(file!(), line!(), column!()),
            || {
                cranpose_ui::widgets::popup::PopupHost(|| {
                    let stage = rememberMutableStateOf(|| 0u8);
                    dialog_action("Open preferences", stage, 1);
                    if stage.get() > 0 {
                        Dialog(
                            DialogSpec::default(),
                            move |_| stage.set(0),
                            move || {
                                Column(Modifier::empty(), ColumnSpec::default(), move || {
                                    dialog_action("Open confirmation", stage, 2);
                                    if stage.get() == 2 {
                                        Dialog(
                                            DialogSpec::default(),
                                            move |_| stage.set(1),
                                            move || {
                                                dialog_action("Finish", stage, 1);
                                            },
                                        );
                                    }
                                });
                            },
                        );
                    }
                });
            },
        );
        shell.set_semantics_enabled(reader);
        shell.update();
        for (key, expected) in [
            (KeyCode::Tab, "Open preferences"),
            (KeyCode::Enter, "Open confirmation"),
            (KeyCode::Enter, "Finish"),
            (KeyCode::Enter, "Open confirmation"),
            (KeyCode::Escape, "Open preferences"),
        ] {
            assert!(shell.on_key_event(&KeyEvent::key_down(key, "")));
            shell.update();
            assert_eq!(focused_description(&mut shell).as_deref(), Some(expected));
        }
    }
}

fn dialog_action(label: &'static str, stage: MutableState<u8>, next: u8) {
    Button(
        Modifier::empty()
            .size(Size::new(220.0, 48.0))
            .content_description(label),
        ButtonSpec::default(),
        move || stage.set(next),
        move || {
            Text(label, Modifier::empty(), TextStyle::default());
        },
    );
}

#[test]
fn described_images_expose_the_image_role_and_preserve_explicit_control_roles() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                for (description, override_description, role) in [
                    (Some("Receipt photo"), None, None),
                    (
                        Some("Fallback description"),
                        Some("Open receipt"),
                        Some(cranpose_ui::SemanticsWidgetRole::Button),
                    ),
                    (None, Some("Lens preview"), None),
                    (None, None, None),
                ] {
                    let bitmap = cranpose_ui::ImageBitmap::from_rgba8(1, 1, vec![255; 4])
                        .expect("one pixel");
                    let modifier =
                        Modifier::empty()
                            .size(Size::new(48.0, 48.0))
                            .semantics(move |config| {
                                if let Some(role) = role {
                                    config.role = Some(role);
                                }
                                if let Some(description) = override_description {
                                    config.content_description = Some(description.into());
                                }
                            });
                    cranpose_ui::Image(
                        bitmap,
                        description.map(String::from),
                        modifier,
                        cranpose_ui::Alignment::CENTER,
                        cranpose_ui::ContentScale::Fit,
                        1.0,
                        None,
                    );
                }
            });
        },
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let root = shell.semantics_tree().expect("image semantics").root();
    for (label, role) in [
        ("Receipt photo", cranpose_ui::SemanticsWidgetRole::Image),
        ("Open receipt", cranpose_ui::SemanticsWidgetRole::Button),
        ("Lens preview", cranpose_ui::SemanticsWidgetRole::Image),
    ] {
        assert_eq!(
            find_semantics_described(root, label)
                .expect(label)
                .widget_role,
            Some(role)
        );
    }
    assert!(find_semantics_described(root, "Fallback description").is_none());
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.widget_role == Some(cranpose_ui::SemanticsWidgetRole::Image) {
            assert!(
                node.description
                    .as_ref()
                    .is_some_and(|label| !label.is_empty()),
                "Decorative images must not create unnamed image semantics"
            );
        }
        pending.extend(node.children.iter());
    }
}

#[test]
fn an_unfocused_app_without_a_reader_keeps_semantics_lazy() {
    let _guard = test_guard();
    let (mut shell, _) = overlapping_shell();
    shell.update();
    assert!(shell.surfaces[0].semantics_tree.is_none());
}

#[test]
fn zero_sized_modal_does_not_hide_visible_controls() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                focus_box("Visible control", 100.0)();
                Box(
                    Modifier::empty()
                        .size(Size::ZERO)
                        .semantics(|config| config.is_modal = true),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let retained = shell.semantics_tree().expect("retained semantics");
    assert!(find_semantics_described(retained.root(), "Visible control").is_some());
    let layout = shell.layout_tree().expect("layout").clone();
    let projected = shell
        .app_context()
        .enter(|| cranpose_ui::build_semantics_tree_from_layout_tree(&layout));
    assert!(find_semantics_described(projected.root(), "Visible control").is_some());
}

#[test]
fn native_snapshot_revision_survives_an_earlier_semantics_read() {
    let _guard = test_guard();
    let (mut shell, _) = overlapping_shell();
    shell.set_semantics_enabled(true);
    shell.update();
    shell.semantics_tree().expect("initial semantics");
    let before = shell.semantics_snapshot_revision();
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
    shell.update();
    shell
        .semantics_tree()
        .expect("semantics read before native bridge");
    assert_ne!(shell.semantics_snapshot_revision(), before);
}

#[test]
fn keyboard_focus_reveals_an_offscreen_control() {
    let _guard = test_guard();
    for horizontal in [false, true] {
        for reverse in [false, true] {
            let (mut shell, scroll) = scrolling_controls(horizontal, reverse);
            assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
            shell.update();
            assert_control_visible(&mut shell, "First", horizontal);
            assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
            shell.update();
            assert_control_visible(&mut shell, "Add page", horizontal);
            let scroll = scroll.borrow().expect("scroll state");
            let manual = if reverse { scroll.max_value() } else { 0.0 };
            scroll.scroll_to(manual);
            shell.update();
            assert_eq!(
                scroll.value(),
                manual,
                "focus must not override subsequent scrolling"
            );
        }
    }
}

#[test]
fn keyboard_reaches_every_item_in_a_lazy_list() {
    let _guard = test_guard();
    for item_height in [48.0, 120.0] {
        let mut shell = AppShell::new(
            HitGraphRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                LazyColumn(
                    Modifier::empty().size(Size::new(240.0, 120.0)),
                    rememberLazyListState(),
                    LazyColumnSpec::default(),
                    |scope| {
                        scope.items(
                            cranpose_foundation::lazy::LazyItems::new(10),
                            move |index| {
                                Text(
                                    format!("Receipt {index}"),
                                    Modifier::empty()
                                        .size(Size::new(200.0, item_height))
                                        .clickable(|_| {}),
                                    TextStyle::default(),
                                );
                            },
                        );
                    },
                );
            },
        );
        shell.update();
        for index in 0..10 {
            assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
            shell.update();
            let focused = shell.app_context().enter(cranpose_ui::active_focus_target);
            let label = format!("Receipt {index}");
            assert_eq!(
                focused,
                Some(reader_control_id(&mut shell, &label)),
                "{label}"
            );
            assert_control_visible(&mut shell, &label, false);
        }
    }
}

fn assert_control_visible(shell: &mut AppShell<HitGraphRenderer>, label: &str, horizontal: bool) {
    let target = reader_control_id(shell, label);
    let (x, y, width, height) = shell.node_layout_bounds(target).expect("focused bounds");
    let (start, size, extent) = if horizontal {
        (x, width, 240.0)
    } else {
        (y, height, 120.0)
    };
    assert!(
        start >= -0.001 && start + size <= extent + 0.001,
        "{label}: start={start}, size={size}, horizontal={horizontal}"
    );
}

fn scrolling_controls(
    horizontal: bool,
    reverse: bool,
) -> (AppShell<HitGraphRenderer>, Rc<RefCell<Option<ScrollState>>>) {
    let scroll = Rc::new(RefCell::new(None));
    let recorded_scroll = Rc::clone(&scroll);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let content_scroll =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            *recorded_scroll.borrow_mut() = Some(content_scroll);
            let modifier = Modifier::empty().size(Size::new(240.0, 120.0));
            let content = move || {
                Box(
                    Modifier::empty()
                        .size(Size::new(200.0, 48.0))
                        .content_description("First")
                        .clickable(|_| {}),
                    BoxSpec::default(),
                    || {},
                );
                Spacer(if horizontal {
                    Size::new(400.0, 1.0)
                } else {
                    Size::new(1.0, 400.0)
                });
                Box(
                    Modifier::empty()
                        .size(Size::new(200.0, 48.0))
                        .content_description("Add page")
                        .clickable(|_| {}),
                    BoxSpec::default(),
                    || {},
                );
            };
            if horizontal {
                Row(
                    modifier.horizontal_scroll(content_scroll, reverse),
                    RowSpec::default(),
                    content,
                );
            } else {
                Column(
                    modifier.vertical_scroll(content_scroll, reverse),
                    ColumnSpec::default(),
                    content,
                );
            }
        },
    );
    shell.update();
    (shell, scroll)
}

#[test]
fn reader_activation_enters_text_editing_without_moving_the_selection() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {
            let field = cranpose_core::remember(|| {
                let field = cranpose_foundation::text::TextFieldState::new("Receipt");
                field.set_selection(cranpose_foundation::text::TextRange::new(2, 4));
                field
            })
            .with(|field| *field);
            cranpose_ui::BasicTextField(
                field,
                Modifier::empty()
                    .size(Size::new(200.0, 48.0))
                    .content_description("Merchant"),
                TextStyle::default(),
            );
        },
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let placed =
        crate::placed_semantics::placed_semantics_from_shell(&mut shell).expect("placed field");
    let field = placed
        .flatten()
        .into_iter()
        .find(|node| node.editable_text && node.label.as_deref() == Some("Merchant"))
        .expect("named editable field");
    assert!(shell.accessibility_activate(field.node_id, None));
    shell.update();
    let editor = shell.ime_editor_state().expect("active native editor");
    assert_eq!(editor.text, "Receipt");
    assert_eq!((editor.selection_start, editor.selection_end), (2, 4));
}

#[test]
fn reader_reveal_scrolls_both_axes_without_starting_keyboard_focus() {
    let _guard = test_guard();
    for horizontal in [false, true] {
        for reverse in [false, true] {
            let (mut shell, scroll) = scrolling_controls(horizontal, reverse);
            shell.set_semantics_enabled(true);
            for label in ["First", "Add page", "First"] {
                let target = reader_control_id(&mut shell, label);
                shell.accessibility_reveal(target);
                shell.update();
                assert_control_visible(&mut shell, label, horizontal);
                assert_eq!(
                    shell.app_context().enter(cranpose_ui::active_focus_target),
                    None
                );
                assert!(!shell.accessibility_reveal(target));
            }
            let scroll = scroll.borrow().expect("scroll state");
            let manual = scroll.max_value() / 2.0;
            scroll.scroll_to(manual);
            shell.update();
            assert_eq!(scroll.value(), manual);
        }
    }
}

struct LazyEditorFixture {
    shell: AppShell<HitGraphRenderer>,
    list: cranpose_foundation::lazy::LazyListState,
    show_editor: cranpose_core::MutableState<bool>,
    active_effects: Rc<Cell<usize>>,
}

fn lazy_editor_fixture(horizontal: bool, reverse: bool) -> LazyEditorFixture {
    lazy_editor_fixture_with_rows(horizontal, reverse, 20)
}

fn lazy_editor_fixture_with_rows(
    horizontal: bool,
    reverse: bool,
    rows: usize,
) -> LazyEditorFixture {
    let captured = Rc::new(RefCell::new(None));
    let state = Rc::clone(&captured);
    let active_effects = Rc::new(Cell::new(0usize));
    let effects = Rc::clone(&active_effects);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let list = rememberLazyListState();
            let show_editor = rememberMutableStateOf(|| true);
            *captured.borrow_mut() = Some((list, show_editor));
            let effects = Rc::clone(&effects);
            let content = move |scope: &mut cranpose_foundation::lazy::LazyListIntervalContent| {
                if show_editor.get() {
                    scope.item_keyed(Some(1), None, move || {
                        let effects = Rc::clone(&effects);
                        cranpose_core::DisposableEffect((), move |_| {
                            effects.set(effects.get() + 1);
                            cranpose_core::DisposableEffectResult::new(move || {
                                effects.set(effects.get() - 1);
                            })
                        });
                        let field = cranpose_core::remember(|| {
                            cranpose_foundation::text::TextFieldState::new("Receipt")
                        })
                        .with(|field| *field);
                        cranpose_ui::BasicTextField(
                            field,
                            Modifier::empty()
                                .size(Size::new(200.0, 48.0))
                                .content_description("Merchant"),
                            TextStyle::default(),
                        );
                    });
                }
                scope.items(rows, |index| {
                    Text(
                        format!("Row {index}"),
                        Modifier::empty().size(Size::new(
                            200.0 + (index % 3) as f32 * 50.0,
                            80.0 + (index % 3) as f32 * 20.0,
                        )),
                        TextStyle::default(),
                    );
                });
            };
            let modifier = Modifier::empty().size(Size::new(240.0, 120.0));
            if horizontal {
                cranpose_ui::LazyRow(
                    modifier,
                    list,
                    cranpose_ui::LazyRowSpec {
                        reverse_layout: reverse,
                        ..Default::default()
                    },
                    content,
                );
            } else {
                LazyColumn(
                    modifier,
                    list,
                    LazyColumnSpec {
                        reverse_layout: reverse,
                        ..Default::default()
                    },
                    content,
                );
            }
        },
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let merchant = reader_control_id(&mut shell, "Merchant");
    assert!(shell.accessibility_activate(merchant, None));
    shell.update();
    let (list, show_editor) = state.borrow().expect("list state");
    LazyEditorFixture {
        shell,
        list,
        show_editor,
        active_effects,
    }
}

fn scroll_lazy_editor(fixture: &mut LazyEditorFixture, delta: f32) {
    fixture.shell.app_context().enter(|| {
        cranpose_core::run_in_mutable_snapshot(|| fixture.list.dispatch_scroll_delta(delta))
            .expect("reader scroll");
    });
    fixture.shell.update();
}

#[test]
fn lazy_list_keeps_the_active_editor_when_the_reader_scrolls_away() {
    let _guard = test_guard();
    for horizontal in [false, true] {
        for reverse in [false, true] {
            let mut fixture = lazy_editor_fixture(horizontal, reverse);
            let merchant = reader_control_id(&mut fixture.shell, "Merchant");
            scroll_lazy_editor(&mut fixture, -2800.0);
            assert!(
                fixture.list.first_visible_item_index() > 5,
                "horizontal={horizontal} reverse={reverse} first={}",
                fixture.list.first_visible_item_index()
            );
            assert_eq!(fixture.active_effects.get(), 1);
            assert_eq!(
                fixture
                    .shell
                    .ime_editor_state()
                    .expect("offscreen editor stays active")
                    .text,
                "Receipt"
            );
            assert!(fixture.shell.on_ime_set_selection(7, 7));
            assert!(fixture.shell.on_key_event(&cranpose_ui::KeyEvent::key_down(
                cranpose_ui::KeyCode::Q,
                "Q"
            )));
            assert!(fixture.shell.on_ime_set_selection(8, 8));
            assert!(fixture.shell.on_paste("Q"));
            fixture.shell.update();
            assert_eq!(
                fixture
                    .shell
                    .ime_editor_state()
                    .expect("offscreen editor accepts text")
                    .text,
                "ReceiptQQ"
            );
            fixture
                .shell
                .app_context()
                .enter(|| fixture.list.scroll_to_item(0, 0.0));
            fixture.shell.update();
            assert_eq!(reader_control_id(&mut fixture.shell, "Merchant"), merchant);
            assert_eq!(
                fixture
                    .shell
                    .ime_editor_state()
                    .expect("returned editor stays active")
                    .text,
                "ReceiptQQ"
            );
        }
    }
}

#[test]
fn lazy_list_keeps_the_active_editor_scrolled_far_past_the_items_it_maps_keys_for() {
    let _guard = test_guard();
    let mut fixture = lazy_editor_fixture_with_rows(false, false, 3_000);
    fixture
        .shell
        .app_context()
        .enter(|| fixture.list.scroll_to_item(1_500, 0.0));
    fixture.shell.update();
    assert!(
        fixture.list.first_visible_item_index() >= 1_500,
        "first={}",
        fixture.list.first_visible_item_index()
    );
    assert_eq!(fixture.active_effects.get(), 1);
    assert_eq!(
        fixture
            .shell
            .ime_editor_state()
            .expect("an editor a thousand rows away stays active")
            .text,
        "Receipt"
    );
    fixture.shell.update();
    assert_eq!(
        fixture.active_effects.get(),
        1,
        "the next frame finds the far editor where it last measured it"
    );
}

#[test]
fn reader_reveal_stops_when_a_scroll_action_reports_no_geometric_progress() {
    let _guard = test_guard();
    let calls = Rc::new(Cell::new(0usize));
    let recorded = Rc::clone(&calls);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let recorded = Rc::clone(&recorded);
            Column(
                Modifier::empty()
                    .size(Size::new(240.0, 120.0))
                    .semantics(move |config| {
                        config.vertical_scroll =
                            Some(cranpose_ui::ScrollAxisRange::new(0.0, 1000.0, false));
                        let recorded = Rc::clone(&recorded);
                        config.scroll_by =
                            Some(cranpose_foundation::SemanticsScrollBy::new(move |_, _| {
                                recorded.set(recorded.get() + 1);
                                true
                            }));
                    }),
                ColumnSpec::default(),
                || {
                    Spacer(Size::new(1.0, 240.0));
                    Text(
                        "Target",
                        Modifier::empty()
                            .size(Size::new(100.0, 48.0))
                            .clickable(|_| {}),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let target = reader_control_id(&mut shell, "Target");
    assert!(shell.accessibility_reveal(target));
    assert_eq!(calls.get(), 1);
}

#[test]
fn retained_lazy_editor_remains_readable_and_can_be_revealed() {
    let _guard = test_guard();
    for horizontal in [false, true] {
        for reverse in [false, true] {
            let mut fixture = lazy_editor_fixture(horizontal, reverse);
            let merchant = reader_control_id(&mut fixture.shell, "Merchant");
            scroll_lazy_editor(&mut fixture, -2800.0);
            assert_eq!(reader_control_id(&mut fixture.shell, "Merchant"), merchant);
            assert!(fixture.shell.accessibility_reveal(merchant));
            fixture.shell.update();
            assert_control_visible(&mut fixture.shell, "Merchant", horizontal);
            assert_eq!(fixture.active_effects.get(), 1);
        }
    }
}

#[test]
fn lazy_list_releases_the_offscreen_editor_after_focus_is_cleared() {
    let _guard = test_guard();
    let mut fixture = lazy_editor_fixture(false, false);
    scroll_lazy_editor(&mut fixture, -1200.0);
    fixture.shell.app_context().enter(|| {
        cranpose_core::run_in_mutable_snapshot(|| cranpose_ui::FocusManager.clear_focus())
            .expect("clear focus");
    });
    scroll_lazy_editor(&mut fixture, -80.0);
    assert!(fixture.shell.ime_editor_state().is_none());
    assert_eq!(fixture.active_effects.get(), 0);
}

#[test]
fn lazy_list_does_not_retain_an_editor_removed_from_content() {
    let _guard = test_guard();
    let mut fixture = lazy_editor_fixture(false, false);
    scroll_lazy_editor(&mut fixture, -1200.0);
    fixture.show_editor.set(false);
    fixture.shell.update();
    assert!(
        fixture.shell.ime_editor_state().is_none(),
        "active effects={}",
        fixture.active_effects.get()
    );
    assert_eq!(fixture.active_effects.get(), 0);
}

#[test]
fn reader_focus_reveals_other_controls_without_replacing_the_active_editor() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {
            let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
            Column(
                Modifier::empty()
                    .size(Size::new(240.0, 120.0))
                    .vertical_scroll(scroll, false),
                ColumnSpec::default(),
                || {
                    for (label, value) in [("Merchant", "Receipt"), ("Payment", "Card")] {
                        let field = cranpose_core::remember(|| {
                            let field = cranpose_foundation::text::TextFieldState::new(value);
                            field.set_selection(cranpose_foundation::text::TextRange::new(2, 4));
                            field
                        })
                        .with(|field| *field);
                        cranpose_ui::BasicTextField(
                            field,
                            Modifier::empty()
                                .size(Size::new(200.0, 48.0))
                                .content_description(label),
                            TextStyle::default(),
                        );
                        Spacer(Size::new(1.0, 200.0));
                    }
                },
            );
        },
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let merchant = reader_control_id(&mut shell, "Merchant");
    let payment = reader_control_id(&mut shell, "Payment");
    shell.accessibility_reveal(merchant);
    shell.update();
    assert!(
        shell.ime_editor_state().is_none(),
        "Reader navigation must not start text entry"
    );
    assert!(shell.accessibility_activate(merchant, None));
    shell.update();
    let before = shell.ime_editor_state().expect("active merchant editor");
    assert!(shell.accessibility_reveal(payment));
    assert_control_visible(&mut shell, "Payment", false);
    shell.update();
    let after = shell
        .ime_editor_state()
        .expect("reader navigation retains the editor");
    assert_eq!(after.text, before.text);
    assert_eq!((after.selection_start, after.selection_end), (2, 4));
    assert_control_visible(&mut shell, "Payment", false);
    assert!(!shell.accessibility_reveal(NodeId::MAX));
    assert!(shell.accessibility_activate(payment, None));
    shell.update();
    assert_eq!(
        shell.ime_editor_state().expect("payment editor").text,
        "Card"
    );
}

fn reader_activation_case(
    content: impl Fn(MutableState<i32>, Rc<Cell<i32>>) + 'static,
) -> (AppShell<HitGraphRenderer>, MutableState<i32>, Rc<Cell<i32>>) {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let mode = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&mode);
    let shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let state = rememberMutableStateOf(|| 0);
            *captured.borrow_mut() = Some(state);
            content(state, Rc::clone(&recorded));
        },
    );
    let state = mode.borrow().expect("control state");
    (shell, state, count)
}

#[test]
fn direct_activation_uses_current_callback_without_pointer_events() {
    let _guard = test_guard();
    let (mut shell, mode, count) = reader_activation_case(|state, recorded| {
        let mode = state.get();
        if mode == 3 {
            return;
        }
        let action = cranpose_foundation::SemanticsCustomAction::new("Activate", move || {
            recorded.set(recorded.get() + mode + 1);
        });
        Box(
            Modifier::empty()
                .size(Size::new(100.0, 48.0))
                .clickable(|_| panic!("semantic activation must not synthesize pointer input"))
                .semantics(move |config| {
                    config.content_description = Some("Direct".into());
                    config.on_click = Some(action.clone());
                    config.enabled = mode != 1;
                    config.hidden = mode == 2;
                }),
            BoxSpec::default(),
            || {},
        );
    });
    shell.update();
    let node_id = reader_control_id(&mut shell, "Direct");
    assert!(shell.accessibility_activate(node_id, None));
    assert_eq!(count.get(), 1);
    mode.set(4);
    shell.update();
    assert!(shell.accessibility_activate(node_id, None));
    assert_eq!(count.get(), 6);
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Enter, "\r")));
    assert_eq!(count.get(), 11);
    for value in 1..=3 {
        mode.set(value);
        shell.update();
        assert!(
            !shell.accessibility_activate(node_id, None),
            "state {value}"
        );
    }
    assert_eq!(count.get(), 11);
}

fn overlapping_shell() -> (AppShell<HitGraphRenderer>, Rc<Cell<i32>>) {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let recorded = Rc::clone(&recorded);
            Box(Modifier::empty(), BoxSpec::default(), move || {
                for (name, increment) in [("Rear", 1), ("Front", 10)] {
                    let recorded = Rc::clone(&recorded);
                    Box(
                        Modifier::empty()
                            .size(Size::new(100.0, 48.0))
                            .content_description(name)
                            .clickable(move |_| recorded.set(recorded.get() + increment)),
                        BoxSpec::default(),
                        || {},
                    );
                }
            });
        },
    );
    (shell, count)
}

#[test]
fn reader_activation_targets_identity_and_preserves_pointer_position() {
    let _guard = test_guard();
    let (mut shell, count) = overlapping_shell();
    shell.update();
    let rear = reader_control_id(&mut shell, "Rear");
    let front = reader_control_id(&mut shell, "Front");
    shell.set_cursor(350.0, 250.0);
    assert!(shell.accessibility_activate(rear, None));
    assert_eq!(count.get(), 1);
    assert!(shell.primary().accessibility_activate(front, None));
    assert_eq!(count.get(), 11);
    assert_eq!(shell.surfaces[0].cursor, (350.0, 250.0));
    assert!(!shell.accessibility_activate(usize::MAX, None));
    assert!(!shell.accessibility_activate(rear, Some(999)));
    assert_eq!(count.get(), 11);
}

#[test]
fn keyboard_activation_targets_focused_control_without_a_screen_reader() {
    let _guard = test_guard();
    let (mut shell, count) = overlapping_shell();
    shell.update();
    assert!(!shell.semantics_active());
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Enter, "\r")));
    assert_eq!(count.get(), 1);
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Space, " ")));
    assert_eq!(count.get(), 11);
    assert!(!shell.semantics_active());
}

#[test]
fn reader_activation_revalidates_disabled_hidden_and_removed_controls() {
    let _guard = test_guard();
    let (mut shell, mode, count) = reader_activation_case(|state, recorded| {
        if state.get() == 3 {
            return;
        }
        Box(
            Modifier::empty()
                .size(Size::new(100.0, 48.0))
                .content_description("Action")
                .clickable(move |_| recorded.set(recorded.get() + 1))
                .semantics(move |config| {
                    config.enabled = state.get() != 1;
                    config.hidden = state.get() == 2;
                }),
            BoxSpec::default(),
            || {},
        );
    });
    shell.update();
    let node_id = reader_control_id(&mut shell, "Action");
    assert!(shell.accessibility_activate(node_id, None));
    for value in 1..=3 {
        mode.set(value);
        shell.update();
        assert!(
            !shell.accessibility_activate(node_id, None),
            "state {value}"
        );
    }
    assert_eq!(count.get(), 1);
}

#[test]
fn reader_activation_resolves_canvas_key_at_its_current_position() {
    let _guard = test_guard();
    let positions = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&positions);
    let state = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&state);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let position = rememberMutableStateOf(|| 10.0f32);
            *captured.borrow_mut() = Some(position);
            let x = position.get();
            let recorded = Rc::clone(&recorded);
            Box(
                Modifier::empty()
                    .size(Size::new(200.0, 100.0))
                    .content_description("Drawing")
                    .clickable(move |point| recorded.borrow_mut().push(point))
                    .semantics(move |config| {
                        let bounds = Rect {
                            x,
                            y: 5.0,
                            width: 20.0,
                            height: 30.0,
                        };
                        let mut child = cranpose_foundation::CanvasSemanticsNode::control(
                            7,
                            bounds,
                            "Drawn button",
                        );
                        child.enabled = x < 100.0;
                        config.canvas_children = vec![child];
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
    shell.update();
    let node_id = reader_control_id(&mut shell, "Drawing");
    assert!(shell.accessibility_activate(node_id, Some(7)));
    state.borrow().expect("position").set(50.0);
    shell.update();
    assert!(shell.accessibility_activate(node_id, Some(7)));
    assert_eq!(
        *positions.borrow(),
        vec![Point::new(20.0, 20.0), Point::new(60.0, 20.0)]
    );
    state.borrow().expect("position").set(110.0);
    shell.update();
    assert!(!shell.accessibility_activate(node_id, Some(7)));
    assert_eq!(positions.borrow().len(), 2);
}

#[test]
fn reader_activation_rejects_a_background_identity_after_a_modal_opens() {
    let _guard = test_guard();
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let state = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&state);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let open = rememberMutableStateOf(|| false);
            *captured.borrow_mut() = Some(open);
            let recorded = Rc::clone(&recorded);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let recorded = Rc::clone(&recorded);
                Text(
                    "Background",
                    Modifier::empty()
                        .size(Size::new(120.0, 48.0))
                        .clickable(move |_| recorded.set(recorded.get() + 1)),
                    TextStyle::default(),
                );
                if open.get() {
                    Box(
                        Modifier::empty()
                            .size(Size::new(120.0, 48.0))
                            .semantics(|config| config.is_modal = true),
                        BoxSpec::default(),
                        || {},
                    );
                }
            });
        },
    );
    shell.update();
    let node_id = reader_control_id(&mut shell, "Background");
    state.borrow().expect("modal").set(true);
    shell.update();
    assert!(!shell.accessibility_activate(node_id, None));
    assert_eq!(count.get(), 0);
    state.borrow().expect("modal").set(false);
    shell.update();
    assert!(shell.accessibility_activate(node_id, None));
    assert_eq!(count.get(), 1);
}

#[test]
fn a_weighted_row_child_keeps_its_place_in_reading_order() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {
            Row(
                Modifier::empty()
                    .size(Size::new(300.0, 80.0))
                    .graphics_layer(|| cranpose_ui_graphics::GraphicsLayer {
                        alpha: 0.9,
                        ..Default::default()
                    })
                    .padding(12.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                || {
                    Row(
                        Modifier::empty().weight(1.0).clickable(|_| {}),
                        RowSpec::new(),
                        || {
                            cranpose_ui::Column(
                                Modifier::empty(),
                                cranpose_ui::ColumnSpec::new(),
                                || {
                                    Text("Idea Marketi", Modifier::empty(), TextStyle::default());
                                    Text("Jul 22", Modifier::empty(), TextStyle::default());
                                },
                            );
                        },
                    );
                    cranpose_ui::Column(Modifier::empty(), cranpose_ui::ColumnSpec::new(), || {
                        Text("6,000.00", Modifier::empty(), TextStyle::default());
                        Text("Check", Modifier::empty(), TextStyle::default());
                        Text("Weak", Modifier::empty(), TextStyle::default());
                    });
                    Box(
                        Modifier::empty()
                            .size(Size::new(48.0, 48.0))
                            .content_description("Row actions")
                            .clickable(|_| {}),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let placed =
        crate::placed_semantics::placed_semantics_from_shell(&mut shell).expect("placed row");
    let order: Vec<_> = placed
        .flatten()
        .into_iter()
        .filter_map(|node| node.label.as_deref())
        .filter(|label| ["Idea Marketi, Jul 22", "6,000.00", "Row actions"].contains(label))
        .collect();
    assert_eq!(order, ["Idea Marketi, Jul 22", "6,000.00", "Row actions"]);
    let amount = placed
        .flatten()
        .into_iter()
        .find(|node| node.label.as_deref() == Some("6,000.00"))
        .expect("amount text");
    assert!(
        amount.layout_bounds.width < 150.0,
        "the amount keeps its own bounds: {:?}",
        amount.layout_bounds
    );
}
