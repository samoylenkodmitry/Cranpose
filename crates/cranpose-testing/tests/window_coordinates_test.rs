use cranpose_app_shell::AppShell;
use cranpose_testing::{
    robot::TestRenderer,
    robot_assertions::{find_semantics_node as find, semantics_contain_text as contains_text},
};
use cranpose_ui::{LazyListScope, SemanticsNode, SemanticsRole};

fn has_text(shell: &mut AppShell<TestRenderer>, value: &str) -> bool {
    contains_text(shell.semantics_tree().expect("semantics").root(), value)
}

fn transformed_modifier(
    width: f32,
    height: f32,
    scale: impl Fn() -> f32 + 'static,
) -> cranpose_ui::Modifier {
    cranpose_ui::Modifier::empty()
        .size_points(width, height)
        .graphics_layer(move || cranpose_ui::GraphicsLayer {
            scale: scale(),
            translation_x: 20.0,
            translation_y: 30.0,
            transform_origin: cranpose_ui::TransformOrigin::new(0.0, 0.0),
            ..cranpose_ui::GraphicsLayer::default()
        })
}

#[test]
fn reported_window_bounds_reach_a_scaled_hover_target() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use cranpose_ui::{
        Box, BoxSpec, Modifier, Rect, Text, TextStyle, collect_is_hovered_as_state,
        rememberMutableInteractionSource,
    };
    let bounds = Rc::new(Cell::new(Rect::from_size(cranpose_ui::Size::default())));
    let report = bounds.clone();
    let mouse = Rc::new(RefCell::new(None));
    let mouse_slot = mouse.clone();
    let scale = Rc::new(Cell::new(None::<cranpose_core::MutableState<f32>>));
    let scale_slot = scale.clone();
    let mut shell = AppShell::new_with_size(
        TestRenderer::default(),
        cranpose_core::location_key(file!(), line!(), column!()),
        move || {
            let report = report.clone();
            let current_scale = cranpose_core::rememberMutableStateOf(|| 0.5);
            scale_slot.set(Some(current_scale));
            *mouse_slot.borrow_mut() = Some(cranpose_ui::mouse_input::local_mouse_input());
            Box(
                transformed_modifier(200.0, 200.0, move || current_scale.get()),
                BoxSpec::default(),
                move || {
                    let interaction = rememberMutableInteractionSource();
                    let hovered = collect_is_hovered_as_state(&interaction);
                    Box(
                        Modifier::empty()
                            .offset(40.0, 20.0)
                            .size_points(40.0, 40.0)
                            .report_window_rect(report.clone())
                            .hoverable(interaction, true),
                        BoxSpec::default(),
                        move || {
                            Text(
                                if hovered.get() { "hovered" } else { "idle" },
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                },
            );
        },
        (200, 200),
        (200.0, 200.0),
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let mouse = mouse.borrow().as_ref().expect("app mouse input").clone();
    for value in [0.5, 1.0] {
        shell.app_context().enter(|| {
            scale.get().expect("layer scale").set(value);
        });
        shell.update();
        let rect = bounds.get();
        mouse.move_to(
            cranpose_ui::mouse_input::MouseInputTarget::Primary,
            cranpose_ui::Point {
                x: rect.x + rect.width / 2.0,
                y: rect.y + rect.height / 2.0,
            },
        );
        shell.update();
        assert!(
            has_text(&mut shell, "hovered"),
            "window coordinates reach the displayed target at scale {value}"
        );
        mouse.move_to(
            cranpose_ui::mouse_input::MouseInputTarget::Primary,
            cranpose_ui::Point { x: 190.0, y: 190.0 },
        );
        shell.update();
        assert!(
            has_text(&mut shell, "idle"),
            "injected mouse movement also dispatches exit"
        );
    }
}

fn focused_field(scale: f32, decorated: bool) -> AppShell<TestRenderer> {
    use cranpose_foundation::text::TextFieldState;
    use cranpose_ui::{
        BasicTextField, BasicTextFieldDecorated, BasicTextFieldOptions, Box, BoxSpec, Modifier,
        TextStyle,
    };
    let mut shell = AppShell::new_with_size(
        TestRenderer::default(),
        cranpose_core::location_key(file!(), line!(), column!()),
        move || {
            Box(
                transformed_modifier(200.0, 100.0, move || scale),
                BoxSpec::default(),
                move || {
                    let state =
                        cranpose_core::remember(|| TextFieldState::new("ABC")).with(|value| *value);
                    let modifier = Modifier::empty().size_points(180.0, 40.0);
                    if decorated {
                        BasicTextFieldDecorated(
                            state,
                            modifier,
                            BasicTextFieldOptions::default(),
                            |scope| {
                                scope.inner_text_field();
                            },
                        );
                    } else {
                        BasicTextField(state, modifier, TextStyle::default());
                    }
                },
            );
        },
        (300, 200),
        (300.0, 200.0),
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let field = find(
        shell.semantics_tree().expect("field semantics").root(),
        &|node| node.details().editable_text,
    )
    .expect("editable field")
    .node_id;
    assert!(shell.accessibility_activate(field, None));
    shell.update();
    shell
}

#[test]
fn native_caret_geometry_follows_the_rendered_field_scale() {
    let mut normal = focused_field(1.0, false);
    let mut scaled = focused_field(0.5, false);
    let normal = normal.ime_caret_geometry().expect("normal caret geometry");
    let scaled = scaled.ime_caret_geometry().expect("scaled caret geometry");
    assert!(
        (scaled.caret_rect(0).height - normal.caret_rect(0).height * 0.5).abs() < 0.01,
        "native IME caret height follows the displayed text"
    );
}

#[test]
fn scaled_scroll_container_reveals_a_target_in_window_coordinates() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use cranpose_ui::{
        Box, BoxSpec, LazyColumn, LazyColumnSpec, LazyItems, Modifier, Rect, Size, Text, TextStyle,
        rememberLazyListState,
    };
    let target = Rc::new(Cell::new(Rect::from_size(Size::default())));
    let responder = Rc::new(RefCell::new(None::<cranpose_ui::BringIntoViewResponder>));
    let content_target = target.clone();
    let content_responder = responder.clone();
    let mut shell = AppShell::new_with_size(
        TestRenderer::default(),
        cranpose_core::location_key(file!(), line!(), column!()),
        move || {
            let target = content_target.clone();
            let responder = content_responder.clone();
            Box(
                transformed_modifier(200.0, 100.0, || 0.5),
                BoxSpec::default(),
                move || {
                    let target = target.clone();
                    let responder = responder.clone();
                    LazyColumn(
                        Modifier::empty().fill_max_size(),
                        rememberLazyListState(),
                        LazyColumnSpec::new(),
                        move |scope| {
                            let target = target.clone();
                            scope.items(LazyItems::new(20), move |index| {
                                *responder.borrow_mut() =
                                    cranpose_ui::local_bring_into_view_responder().current();
                                let modifier = Modifier::empty().height(32.0);
                                let modifier = if index == 2 {
                                    modifier.report_window_rect(target.clone())
                                } else {
                                    modifier
                                };
                                Text(format!("row {index}"), modifier, TextStyle::default());
                            });
                        },
                    );
                },
            );
        },
        (300, 200),
        (300.0, 200.0),
    );
    let context = Rc::clone(shell.app_context());
    context.enter(|| {
        responder
            .borrow()
            .as_ref()
            .expect("scroll responder")
            .bring_into_view(target.get(), 0.0);
    });
    shell.update();
    let target = target.get();
    assert!(
        target.y + target.height <= 80.0 - 12.0 + 0.1,
        "scaled scrolling puts the target above the visible bottom margin: {target:?}"
    );
}

#[test]
fn decorated_field_exposes_pasted_text_to_accessibility() {
    let mut shell = focused_field(0.5, true);
    assert!(shell.on_paste("Z"));
    shell.update();
    let tree = shell.semantics_tree().expect("field semantics");
    let field = find(tree.root(), &|node| node.details().editable_text).expect("editable field");
    assert!(field.text.as_deref().is_some_and(|text| text.contains('Z')));
    assert!(
        !field.actions.is_empty(),
        "the editable field exposes its actions"
    );
    assert_eq!(
        editable_fields(tree.root()),
        1,
        "accessibility exposes one editable field"
    );
}

fn editable_fields(node: &SemanticsNode) -> usize {
    usize::from(node.details().editable_text)
        + node.children.iter().map(editable_fields).sum::<usize>()
}

#[test]
fn text_preserves_its_explicit_accessibility_description() {
    let mut shell = AppShell::new_with_size(
        TestRenderer::default(),
        cranpose_core::location_key(file!(), line!(), column!()),
        || {
            cranpose_ui::Text(
                "264.000",
                cranpose_ui::Modifier::empty().semantics(|config| {
                    config.content_description = Some("Selected quote price".to_string());
                }),
                cranpose_ui::TextStyle::default(),
            );
        },
        (300, 200),
        (300.0, 200.0),
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let tree = shell.semantics_tree().expect("text semantics");
    let text = find(tree.root(), &|node| {
        node.description.as_deref() == Some("Selected quote price")
    })
    .expect("the explicit accessibility label remains available");
    assert!(matches!(&text.role, SemanticsRole::Text { value } if value.as_str() == "264.000"));
}
