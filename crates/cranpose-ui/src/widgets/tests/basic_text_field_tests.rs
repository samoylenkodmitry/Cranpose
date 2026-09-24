use super::*;

#[test]
fn handle_grab_bias_reads_the_live_tip_not_a_snapshot() {
    let tip_y: Rc<Cell<f32>> = Rc::new(Cell::new(100.0));
    let drag_bias: Rc<Cell<Option<HandleGrabOffset>>> = Rc::new(Cell::new(None));
    let grab = {
        let tip_y = Rc::clone(&tip_y);
        let drag_bias = Rc::clone(&drag_bias);
        move |finger_y: f32| {
            track_handle_grab(&drag_bias, HandleKind::SelectionEnd, tip_y.get(), finger_y)
        }
    };

    tip_y.set(148.0);
    let bias = grab(160.0);
    assert_eq!(
        bias,
        148.0 - 160.0,
        "the grab bias must anchor on the handle's CURRENT line"
    );
}
use std::sync::Arc;

use cranpose_core::{Composition, DefaultScheduler, MemoryApplier, Runtime, location_key};

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

fn render_collapsed_handles(direct_manipulation: bool) -> crate::renderer::RecordedRenderScene {
    use cranpose_ui_graphics::Size;

    use crate::{layout::LayoutEngine, renderer::HeadlessRenderer, widgets::PopupHost};

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let state = TextFieldState::new("hello world");

    let mut content = move || {
        PopupHost(move || {
            let controller = TextFieldHandleController::new();
            controller.publish(TextFieldHandleMetrics {
                focused: true,
                direct_manipulation,
                node_origin: Point { x: 0.0, y: 10.0 },
                padding_left: 0.0,
                padding_top: 0.0,
                scroll_offset: 0.0,
                line_height: 18.0,
                glyph_box: (0.0, 18.0),
                wrap_width: None,
            });
            SelectionHandles(
                state,
                TextStyle::default(),
                controller,
                Color(0.0, 0.478, 1.0, 1.0),
            );
        });
    };

    composition.render(key, &mut content).expect("render");
    for _ in 0..16 {
        if !composition.should_render() {
            break;
        }
        composition.reconcile(key, &mut content).expect("reconcile");
    }
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 400.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    drop(applier);
    HeadlessRenderer::new().render(&layout)
}

fn render_range_menu(direct_manipulation: bool) -> crate::renderer::RecordedRenderScene {
    use cranpose_ui_graphics::Size;

    use crate::{layout::LayoutEngine, renderer::HeadlessRenderer, widgets::PopupHost};

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let state = TextFieldState::new("hello world");

    let mut content = move || {
        PopupHost(move || {
            let controller = TextFieldHandleController::new();
            if state.selection() != TextRange::new(0, 5) {
                state.set_selection(TextRange::new(0, 5));
            }
            controller.publish(TextFieldHandleMetrics {
                focused: true,
                direct_manipulation,
                node_origin: Point { x: 0.0, y: 40.0 },
                padding_left: 0.0,
                padding_top: 0.0,
                scroll_offset: 0.0,
                line_height: 18.0,
                glyph_box: (0.0, 18.0),
                wrap_width: None,
            });
            SelectionHandles(
                state,
                TextStyle::default(),
                controller,
                Color(0.0, 0.478, 1.0, 1.0),
            );
        });
    };

    composition.render(key, &mut content).expect("render");
    for _ in 0..16 {
        if !composition.should_render() {
            break;
        }
        composition.reconcile(key, &mut content).expect("reconcile");
    }
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 400.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    drop(applier);
    HeadlessRenderer::new().render(&layout)
}

fn render_range_menu_subcomposed(
    direct_manipulation: bool,
) -> crate::renderer::RecordedRenderScene {
    use cranpose_ui_graphics::Size;

    use crate::{
        layout::LayoutEngine,
        renderer::HeadlessRenderer,
        widgets::{BoxWithConstraints, PopupHost},
    };

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let state = TextFieldState::new("hello world");

    let mut content = move || {
        PopupHost(move || {
            let state = state;
            BoxWithConstraints(
                Modifier::empty().size(Size {
                    width: 300.0,
                    height: 300.0,
                }),
                move |_scope| {
                    let controller = TextFieldHandleController::new();
                    if state.selection() != TextRange::new(0, 5) {
                        state.set_selection(TextRange::new(0, 5));
                    }
                    controller.publish(TextFieldHandleMetrics {
                        focused: true,
                        direct_manipulation,
                        node_origin: Point { x: 0.0, y: 40.0 },
                        padding_left: 0.0,
                        padding_top: 0.0,
                        scroll_offset: 0.0,
                        line_height: 18.0,
                        glyph_box: (0.0, 18.0),
                        wrap_width: None,
                    });
                    SelectionHandles(
                        state,
                        TextStyle::default(),
                        controller,
                        Color(0.0, 0.478, 1.0, 1.0),
                    );
                },
            );
        });
    };

    composition.render(key, &mut content).expect("render");
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut scene = None;
    for _ in 0..8 {
        for _ in 0..16 {
            if !composition.should_render() {
                break;
            }
            composition.reconcile(key, &mut content).expect("reconcile");
        }
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 400.0,
                    height: 400.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        drop(applier);
        scene = Some(HeadlessRenderer::new().render(&layout));
    }
    scene.expect("scene")
}

fn render_range_menu_lazy_column(
    direct_manipulation: bool,
) -> crate::renderer::RecordedRenderScene {
    use cranpose_foundation::lazy::{LazyListScope, rememberLazyListState};
    use cranpose_ui_graphics::Size;

    use crate::{
        LazyColumn, LazyColumnSpec, layout::LayoutEngine, renderer::HeadlessRenderer,
        widgets::PopupHost,
    };

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let state = TextFieldState::new("hello world");

    let mut content = move || {
        PopupHost(move || {
            let state = state;
            let list_state = rememberLazyListState();
            LazyColumn(
                Modifier::empty().size(Size {
                    width: 300.0,
                    height: 300.0,
                }),
                list_state,
                LazyColumnSpec::default(),
                move |scope| {
                    let state = state;
                    scope.items(1, move |_index| {
                        let controller = TextFieldHandleController::new();
                        if state.selection() != TextRange::new(0, 5) {
                            state.set_selection(TextRange::new(0, 5));
                        }
                        controller.publish(TextFieldHandleMetrics {
                            focused: true,
                            direct_manipulation,
                            node_origin: Point { x: 0.0, y: 40.0 },
                            padding_left: 0.0,
                            padding_top: 0.0,
                            scroll_offset: 0.0,
                            line_height: 18.0,
                            glyph_box: (0.0, 18.0),
                            wrap_width: None,
                        });
                        SelectionHandles(
                            state,
                            TextStyle::default(),
                            controller,
                            Color(0.0, 0.478, 1.0, 1.0),
                        );
                    });
                },
            );
        });
    };

    composition.render(key, &mut content).expect("render");
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut scene = None;
    for _ in 0..8 {
        for _ in 0..16 {
            if !composition.should_render() {
                break;
            }
            composition.reconcile(key, &mut content).expect("reconcile");
        }
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 400.0,
                    height: 400.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        drop(applier);
        scene = Some(HeadlessRenderer::new().render(&layout));
    }
    scene.expect("scene")
}

#[test]
fn field_window_origin_follows_vertical_scroll() {
    use std::cell::RefCell;

    use cranpose_core::{Key, remember};
    use cranpose_foundation::modifier_element;
    use cranpose_ui_graphics::Size;

    use crate::{
        layout::{LayoutBox, LayoutEngine, policies::EmptyMeasurePolicy},
        renderer::HeadlessRenderer,
        scroll::ScrollState,
        widgets::{Column, ColumnSpec, Layout, PopupHost, Spacer},
    };

    let _app_context = crate::render_state::app_context_test_scope();

    let mut composition = Composition::new(MemoryApplier::new());
    let state = TextFieldState::new("hello world");
    let controller_slot: Rc<RefCell<Option<TextFieldHandleController>>> =
        Rc::new(RefCell::new(None));
    let scroll_slot: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));

    let spacer_before = 200.0_f32;
    let mut content = {
        let controller_slot = Rc::clone(&controller_slot);
        let scroll_slot = Rc::clone(&scroll_slot);
        move || {
            let controller_slot = Rc::clone(&controller_slot);
            let scroll_slot = Rc::clone(&scroll_slot);
            PopupHost(move || {
                let controller =
                    remember(TextFieldHandleController::new).with(TextFieldHandleController::clone);
                *controller_slot.borrow_mut() = Some(controller.clone());
                let scroll = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
                *scroll_slot.borrow_mut() = Some(scroll);
                let state = state;
                Column(
                    Modifier::empty()
                        .size(Size {
                            width: 300.0,
                            height: 150.0,
                        })
                        .vertical_scroll(scroll, false),
                    ColumnSpec::default(),
                    move || {
                        Spacer(Size {
                            width: 300.0,
                            height: spacer_before,
                        });
                        let element = TextFieldElement::new(state, TextStyle::default())
                            .with_handle_controller(controller.clone());
                        let field_modifier = Modifier::from_parts(vec![modifier_element(element)]);
                        Layout(field_modifier, EmptyMeasurePolicy, || {});
                        Spacer(Size {
                            width: 300.0,
                            height: 400.0,
                        });
                    },
                );
            });
        }
    };

    fn find_field_rect(node: &LayoutBox) -> Option<cranpose_ui_graphics::Rect> {
        if node
            .node_data
            .modifier_slices()
            .text_field_window_origin()
            .is_some()
        {
            return Some(node.rect);
        }
        node.children.iter().find_map(find_field_rect)
    }

    fn layout_and_read(
        composition: &mut Composition<MemoryApplier>,
        key: Key,
        content: &mut dyn FnMut(),
        controller_slot: &Rc<RefCell<Option<TextFieldHandleController>>>,
    ) -> (Point, f32) {
        for _ in 0..16 {
            if !composition.should_render() {
                break;
            }
            composition
                .reconcile(key, &mut *content)
                .expect("reconcile");
        }
        let root = composition.root().expect("root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(
                root,
                cranpose_ui_graphics::Size {
                    width: 400.0,
                    height: 600.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        drop(applier);
        let _ = HeadlessRenderer::new().render(&layout);
        let field_y = find_field_rect(layout.root()).expect("field placed").y;
        let node_origin = controller_slot
            .borrow()
            .as_ref()
            .expect("controller")
            .metrics()
            .expect("metrics published")
            .node_origin;
        (node_origin, field_y)
    }

    let key = location_key(file!(), line!(), column!());
    composition.render(key, &mut content).expect("render");

    let (origin0, field_y0) =
        layout_and_read(&mut composition, key, &mut content, &controller_slot);
    assert!(
        (origin0.y - field_y0).abs() < 0.5,
        "published node_origin.y {} must equal the field's placed window-y {}",
        origin0.y,
        field_y0
    );
    assert!(
        origin0.y >= spacer_before - 0.5,
        "field should start at/after the {spacer_before}px leading spacer, got {}",
        origin0.y
    );

    let scroll = *scroll_slot.borrow().as_ref().expect("scroll state");
    scroll.scroll_to(50.0);
    assert!(
        scroll.value() >= 49.5,
        "test setup: content must be tall enough to scroll 50px (got {})",
        scroll.value()
    );
    let (origin1, field_y1) =
        layout_and_read(&mut composition, key, &mut content, &controller_slot);
    assert!(
        (origin1.y - field_y1).abs() < 0.5,
        "after scroll, node_origin.y {} must still equal the field's placed window-y {}",
        origin1.y,
        field_y1
    );
    assert!(
        (origin1.y - (origin0.y - 50.0)).abs() < 0.5,
        "scrolling 50px must shift the published field origin up by 50px: \
         before {}, after {} (expected {})",
        origin0.y,
        origin1.y,
        origin0.y - 50.0
    );
}

#[test]
fn window_offset_roundtrip_holds_under_scroll_offset() {
    let _app_context = crate::render_state::app_context_test_scope();
    let text = "hello world";
    let style = TextStyle::default();
    for node_origin in [Point { x: 12.0, y: 240.0 }, Point { x: 12.0, y: 190.0 }] {
        let metrics = TextFieldHandleMetrics {
            focused: true,
            direct_manipulation: true,
            node_origin,
            padding_left: 4.0,
            padding_top: 3.0,
            scroll_offset: 0.0,
            line_height: 18.0,
            glyph_box: (0.0, 18.0),
            wrap_width: None,
        };
        for offset in 0..=text.len() {
            if !text.is_char_boundary(offset) {
                continue;
            }
            let tip =
                handle_tip_window_pos(text, &style, &metrics, offset, LineAffinity::Downstream);
            let resolved = window_pos_to_offset(text, &style, &metrics, tip, 0.0);
            assert_eq!(
                resolved, offset,
                "finger at the tip of offset {offset} must map back to it \
                 under origin {node_origin:?}, got {resolved}"
            );
        }
    }
}

#[test]
fn shared_wrap_boundary_anchors_by_handle_affinity() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let text = "aaaaaaaaaaaaaaaaaaaaaaaa";
        let style = TextStyle::default();
        let annotated = crate::text::AnnotatedString::from(text);
        let full = crate::text::measure_text(&annotated, &style);
        let wrap_width = full.width / 3.0;
        let ranges = crate::text::wrapped_line_ranges(
            None,
            &annotated,
            &style,
            crate::text::TextLayoutOptions::default(),
            Some(wrap_width),
        );
        assert!(
            ranges.len() >= 2,
            "test setup: text must wrap, got {ranges:?}"
        );
        let boundary = ranges[1].start;
        assert_eq!(
            ranges[0].end, boundary,
            "test setup: a mid-word wrap must share its boundary byte, got {ranges:?}"
        );

        let line_height = 20.0;
        let metrics = TextFieldHandleMetrics {
            focused: true,
            direct_manipulation: true,
            node_origin: Point { x: 0.0, y: 0.0 },
            padding_left: 0.0,
            padding_top: 0.0,
            scroll_offset: 0.0,
            line_height,
            glyph_box: (0.0, line_height),
            wrap_width: Some(wrap_width),
        };

        let end_tip =
            handle_tip_window_pos(text, &style, &metrics, boundary, LineAffinity::Upstream);
        assert!(
            (end_tip.y - line_height).abs() < 0.5,
            "end handle must sit on the UPPER line's bottom ({line_height}), got y={}",
            end_tip.y
        );
        assert!(
            end_tip.x > 1.0,
            "end handle must sit at the upper line's right edge, got x={}",
            end_tip.x
        );

        let start_tip =
            handle_tip_window_pos(text, &style, &metrics, boundary, LineAffinity::Downstream);
        assert!(
            (start_tip.y - 2.0 * line_height).abs() < 0.5,
            "start handle must sit on the LOWER line's bottom ({}), got y={}",
            2.0 * line_height,
            start_tip.y
        );
        assert!(
            start_tip.x.abs() < 0.5,
            "start handle must sit at the lower line's left edge, got x={}",
            start_tip.x
        );

        let mut grab = HandleGrabOffset::begin(end_tip.y, end_tip.y);
        for finger_y in [
            end_tip.y,
            end_tip.y + 8.0,
            end_tip.y + 32.0,
            end_tip.y + 80.0,
        ] {
            let bias = grab.track(finger_y);
            let resolved = window_pos_to_offset(
                text,
                &style,
                &metrics,
                Point {
                    x: end_tip.x,
                    y: finger_y,
                },
                bias,
            );
            let resolved_tip =
                handle_tip_window_pos(text, &style, &metrics, resolved, LineAffinity::Upstream);
            let target_tip_y =
                (finger_y + bias).clamp(line_height, ranges.len() as f32 * line_height);
            assert!(
                (resolved_tip.y - target_tip_y).abs() <= line_height * 0.5 + 0.5,
                "finger y={finger_y}, bias={bias} resolved to offset {resolved} at y={}, expected the nearest visual-line bottom to {}",
                resolved_tip.y,
                target_tip_y,
            );
        }
    });
}

fn text_values(scene: &crate::renderer::RecordedRenderScene) -> Vec<String> {
    use crate::renderer::RenderOp;
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Text { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn render_caret_action_menu(
    can_paste: bool,
    can_undo: bool,
    can_redo: bool,
) -> crate::renderer::RecordedRenderScene {
    use cranpose_ui_graphics::Size;

    use crate::{layout::LayoutEngine, renderer::HeadlessRenderer, widgets::PopupHost};

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let mut content = move || {
        PopupHost(move || {
            CaretActionMenu(
                40.0,
                60.0,
                true,
                can_paste,
                can_undo,
                can_redo,
                || {},
                || {},
                || {},
                || {},
            );
        });
    };

    composition.render(key, &mut content).expect("render");
    for _ in 0..16 {
        if !composition.should_render() {
            break;
        }
        composition.reconcile(key, &mut content).expect("reconcile");
    }
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 400.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    drop(applier);
    HeadlessRenderer::new().render(&layout)
}

#[test]
fn caret_action_menu_shows_paste_select_all_undo_redo() {
    let _app_context = crate::render_state::app_context_test_scope();

    let all = text_values(&render_caret_action_menu(true, true, true));
    for label in ["Paste", "Select all", "Undo", "Redo"] {
        assert!(
            all.iter().any(|t| t == label),
            "caret menu should show {label:?}, got {all:?}"
        );
    }

    let bare = text_values(&render_caret_action_menu(false, false, false));
    assert!(
        bare.iter().any(|t| t == "Select all"),
        "Select all is always available, got {bare:?}"
    );
    assert!(
        !bare
            .iter()
            .any(|t| t == "Paste" || t == "Undo" || t == "Redo"),
        "Paste/Undo/Redo must be hidden when unavailable, got {bare:?}"
    );
}

#[test]
fn context_menu_shows_for_pointer_selection_on_every_platform() {
    let _app_context = crate::render_state::app_context_test_scope();

    let touch = text_values(&render_range_menu(true));
    assert!(
        touch.iter().any(|t| t == "Copy"),
        "touch selection should show the Copy menu item, got {touch:?}"
    );
    assert!(
        touch.iter().any(|t| t == "Cut"),
        "expected Cut, got {touch:?}"
    );
    assert!(
        touch.iter().any(|t| t == "Select all"),
        "expected Select all, got {touch:?}"
    );

    let mouse = text_values(&render_range_menu(true));
    assert!(
        mouse.iter().any(|t| t == "Copy"),
        "mouse selection must expose the same direct-manipulation menu, got {mouse:?}"
    );
    let keyboard = text_values(&render_range_menu(false));
    assert!(
        !keyboard.iter().any(|t| t == "Copy"),
        "keyboard-only focus must keep a clean caret, got {keyboard:?}"
    );
}

#[test]
fn selection_handles_and_menu_survive_subcomposition() {
    let _app_context = crate::render_state::app_context_test_scope();
    let scene = render_range_menu_subcomposed(true);

    let texts = text_values(&scene);
    assert!(
        texts.iter().any(|t| t == "Copy"),
        "a touch selection inside a subcomposition should show the Copy menu \
         item through the host, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "Select all"),
        "expected Select all inside a subcomposition, got {texts:?}"
    );
    assert_eq!(
        image_count(&scene),
        2,
        "a touch range selection should show two finger teardrop handles in \
         the overlay across the subcomposition boundary"
    );
}

#[test]
fn selection_handles_and_menu_survive_lazy_column_item() {
    let _app_context = crate::render_state::app_context_test_scope();
    let scene = render_range_menu_lazy_column(true);

    let texts = text_values(&scene);
    assert!(
        texts.iter().any(|t| t == "Copy"),
        "a touch selection inside a LazyColumn item should show the Copy menu \
         item through the host, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "Select all"),
        "expected Select all inside a LazyColumn item, got {texts:?}"
    );
    assert_eq!(
        image_count(&scene),
        2,
        "a touch range selection should show two finger teardrop handles in \
         the overlay across the LazyColumn item subcomposition boundary"
    );
}

fn image_count(scene: &crate::renderer::RecordedRenderScene) -> usize {
    use cranpose_ui_graphics::DrawPrimitive;

    use crate::renderer::RenderOp;
    scene
        .operations()
        .iter()
        .filter(|op| {
            matches!(
                op,
                RenderOp::Primitive {
                    primitive: DrawPrimitive::Image { .. },
                    ..
                }
            )
        })
        .count()
}

#[test]
fn cursor_handle_shows_for_pointer_selection_on_every_platform() {
    let _app_context = crate::render_state::app_context_test_scope();
    assert_eq!(
        image_count(&render_collapsed_handles(true)),
        1,
        "a touch caret should show one finger cursor handle in the overlay"
    );
    assert_eq!(
        image_count(&render_collapsed_handles(true)),
        1,
        "a mouse-created caret should expose its draggable handle"
    );
    assert_eq!(
        image_count(&render_collapsed_handles(false)),
        0,
        "keyboard-only focus should keep a clean caret"
    );
}

#[test]
fn basic_text_field_creates_node() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let state = TextFieldState::new("Test content");

    let result = composition.render(location_key(file!(), line!(), column!()), move || {
        BasicTextField(state, Modifier::empty(), TextStyle::default());
    });

    assert!(result.is_ok());
    assert!(composition.root().is_some());
}

#[test]
fn basic_text_field_state_updates() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        assert_eq!(state.text(), "Hello");

        state.edit(|buffer| {
            buffer.place_cursor_at_end();
            buffer.insert("!");
        });

        assert_eq!(state.text(), "Hello!");
    });
}

#[test]
fn lazy_column_responder_scrolls_a_hidden_caret_into_view() {
    use std::cell::RefCell;

    use cranpose_core::Key;
    use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
    use cranpose_ui_graphics::Size;

    use crate::{
        LazyColumn, LazyColumnSpec,
        bring_into_view::local_bring_into_view_responder,
        layout::LayoutEngine,
        renderer::HeadlessRenderer,
        widgets::{Box, BoxSpec, PopupHost},
    };

    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let responder_slot: Rc<RefCell<Option<crate::bring_into_view::BringIntoViewResponder>>> =
        Rc::new(RefCell::new(None));
    let state_slot: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));

    let mut content = {
        let responder_slot = Rc::clone(&responder_slot);
        let state_slot = Rc::clone(&state_slot);
        move || {
            let responder_slot = Rc::clone(&responder_slot);
            let state_slot = Rc::clone(&state_slot);
            PopupHost(move || {
                let list_state = rememberLazyListState();
                *state_slot.borrow_mut() = Some(list_state);
                let responder_slot = Rc::clone(&responder_slot);
                LazyColumn(
                    Modifier::empty().size(Size {
                        width: 300.0,
                        height: 400.0,
                    }),
                    list_state,
                    LazyColumnSpec::default(),
                    move |scope| {
                        let responder_slot = Rc::clone(&responder_slot);
                        scope.items(30, move |_index| {
                            if responder_slot.borrow().is_none()
                                && let Some(r) = local_bring_into_view_responder().current()
                            {
                                *responder_slot.borrow_mut() = Some(r);
                            }
                            Box(
                                Modifier::empty().size(Size {
                                    width: 300.0,
                                    height: 80.0,
                                }),
                                BoxSpec::default(),
                                || {},
                            );
                        });
                    },
                );
            });
        }
    };

    fn run_layout(
        composition: &mut Composition<MemoryApplier>,
        key: Key,
        content: &mut dyn FnMut(),
    ) {
        for _ in 0..16 {
            if !composition.should_render() {
                break;
            }
            composition
                .reconcile(key, &mut *content)
                .expect("reconcile");
        }
        let root = composition.root().expect("root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 400.0,
                    height: 600.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        drop(applier);
        let _ = HeadlessRenderer::new().render(&layout);
    }

    let key = location_key(file!(), line!(), column!());
    composition.render(key, &mut content).expect("render");
    run_layout(&mut composition, key, &mut content);

    let responder = responder_slot
        .borrow()
        .clone()
        .expect("LazyColumn provides a bring-into-view responder to its items");
    let list_state = state_slot.borrow().expect("list state captured");
    let offset0 = list_state.first_visible_item_scroll_offset();
    let index0 = list_state.first_visible_item_index();

    responder.bring_into_view(
        Rect {
            x: 10.0,
            y: 100.0,
            width: 2.0,
            height: 20.0,
        },
        0.0,
    );
    run_layout(&mut composition, key, &mut content);
    assert_eq!(
        list_state.first_visible_item_index(),
        index0,
        "an already-visible caret must not scroll the list"
    );
    assert!(
        (list_state.first_visible_item_scroll_offset() - offset0).abs() < 0.5,
        "an already-visible caret must not scroll the list"
    );

    responder.bring_into_view(
        Rect {
            x: 10.0,
            y: 360.0,
            width: 2.0,
            height: 20.0,
        },
        250.0,
    );
    run_layout(&mut composition, key, &mut content);
    let scrolled_forward = list_state.first_visible_item_index() > index0
        || list_state.first_visible_item_scroll_offset() > offset0 + 0.5;
    assert!(
        scrolled_forward,
        "a caret behind the keyboard must scroll the list forward \
         (index {} -> {}, offset {:.1} -> {:.1})",
        index0,
        list_state.first_visible_item_index(),
        offset0,
        list_state.first_visible_item_scroll_offset(),
    );
}
