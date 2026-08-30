use cranpose_core::{MemoryApplier, NodeId, location_key};
use cranpose_foundation::lazy::{LazyListScope, rememberLazyListState};
use cranpose_ui_graphics::{DrawPrimitive, Point, Rect, Size};

use crate::{
    Composition, LazyColumn, LazyColumnSpec,
    layout::{LayoutEngine, LayoutTree},
    modifier::{Brush, Color, Modifier},
    primitives::{Column, ColumnSpec},
    renderer::{HeadlessRenderer, RenderOp},
    widgets::{Popup, PopupHost},
};

const MARKER: Color = Color(0.9, 0.1, 0.3, 1.0);

fn compute_layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> LayoutTree {
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
    layout
}

fn settle(
    composition: &mut Composition<MemoryApplier>,
    key: cranpose_core::Key,
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
}

fn marker_rects(scene: &crate::renderer::RecordedRenderScene) -> Vec<Rect> {
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Primitive {
                primitive: DrawPrimitive::Rect { rect, brush, .. },
                ..
            }
            | RenderOp::Primitive {
                primitive: DrawPrimitive::RoundRect { rect, brush, .. },
                ..
            } if *brush == Brush::solid(MARKER) => Some(*rect),
            _ => None,
        })
        .collect()
}

#[test]
fn popup_content_renders_outside_its_anchors_parent_bounds() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let mut content = || {
        PopupHost(|| {
            Column(
                Modifier::empty()
                    .size(Size {
                        width: 50.0,
                        height: 50.0,
                    })
                    .clip_to_bounds(),
                ColumnSpec::default(),
                || {
                    Popup(
                        Rect {
                            x: 200.0,
                            y: 300.0,
                            width: 0.0,
                            height: 0.0,
                        },
                        Point { x: 0.0, y: 0.0 },
                        || {
                            Column(
                                Modifier::empty()
                                    .size(Size {
                                        width: 20.0,
                                        height: 20.0,
                                    })
                                    .background(MARKER),
                                ColumnSpec::default(),
                                || {},
                            );
                        },
                    );
                },
            );
        });
    };

    composition
        .render(key, &mut content)
        .expect("initial render");
    settle(&mut composition, key, &mut content);

    let root = composition.root().expect("popup host root");
    let layout = compute_layout(&mut composition, root);
    let scene = HeadlessRenderer::new().render(&layout);

    let rects = marker_rects(&scene);
    assert_eq!(
        rects.len(),
        1,
        "expected exactly one marker rect from the overlay content, got {rects:?}"
    );
    let rect = rects[0];
    assert!(
        rect.x >= 200.0 - 0.5 && rect.x <= 200.0 + 0.5,
        "marker x should be at the anchor (200), was {}",
        rect.x
    );
    assert!(
        rect.y >= 300.0 - 0.5 && rect.y <= 300.0 + 0.5,
        "marker y should be at the anchor (300), was {}",
        rect.y
    );
    assert!(
        rect.x >= 50.0,
        "marker must render outside the anchor's parent bounds (x>=50), was {}",
        rect.x
    );
}

#[test]
fn popup_is_removed_from_overlay_when_no_longer_composed() {
    use cranpose_core::mutableStateOf;

    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let show = mutableStateOf(true);
    let show_for_content = show;
    let mut content = move || {
        let show = show_for_content;
        PopupHost(move || {
            if show.value() {
                Popup(
                    Rect {
                        x: 120.0,
                        y: 60.0,
                        width: 0.0,
                        height: 0.0,
                    },
                    Point { x: 0.0, y: 0.0 },
                    || {
                        Column(
                            Modifier::empty()
                                .size(Size {
                                    width: 10.0,
                                    height: 10.0,
                                })
                                .background(MARKER),
                            ColumnSpec::default(),
                            || {},
                        );
                    },
                );
            }
        });
    };

    composition.render(key, &mut content).expect("render");
    settle(&mut composition, key, &mut content);
    let root = composition.root().expect("root");
    let layout = compute_layout(&mut composition, root);
    let scene = HeadlessRenderer::new().render(&layout);
    assert_eq!(
        marker_rects(&scene).len(),
        1,
        "popup visible while composed"
    );

    show.set(false);
    settle(&mut composition, key, &mut content);
    let root = composition.root().expect("root");
    let layout = compute_layout(&mut composition, root);
    let scene = HeadlessRenderer::new().render(&layout);
    assert_eq!(
        marker_rects(&scene).len(),
        0,
        "popup removed from overlay after it stops being composed"
    );
}

#[test]
fn popup_inside_subcomposition_still_reaches_the_host() {
    use crate::widgets::BoxWithConstraints;

    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let mut content = || {
        PopupHost(|| {
            BoxWithConstraints(
                Modifier::empty().size(Size {
                    width: 200.0,
                    height: 200.0,
                }),
                |_scope| {
                    Popup(
                        Rect {
                            x: 120.0,
                            y: 90.0,
                            width: 0.0,
                            height: 0.0,
                        },
                        Point { x: 0.0, y: 0.0 },
                        || {
                            Column(
                                Modifier::empty()
                                    .size(Size {
                                        width: 15.0,
                                        height: 15.0,
                                    })
                                    .background(MARKER),
                                ColumnSpec::default(),
                                || {},
                            );
                        },
                    );
                },
            );
        });
    };

    composition.render(key, &mut content).expect("render");
    let scene = {
        let mut scene = None;
        for _ in 0..6 {
            settle(&mut composition, key, &mut content);
            let root = composition.root().expect("root");
            let layout = compute_layout(&mut composition, root);
            scene = Some(HeadlessRenderer::new().render(&layout));
        }
        scene.expect("scene")
    };

    let rects = marker_rects(&scene);
    assert_eq!(
        rects.len(),
        1,
        "a Popup composed inside a BoxWithConstraints subcomposition must reach \
         the enclosing PopupHost, got {rects:?}"
    );
    assert!(
        (rects[0].x - 120.0).abs() <= 0.5 && (rects[0].y - 90.0).abs() <= 0.5,
        "overlay marker should paint at the anchor (120,90), was {:?}",
        rects[0]
    );
}

#[test]
fn popup_inside_lazy_column_item_still_reaches_the_host() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let mut content = || {
        PopupHost(|| {
            let state = rememberLazyListState();
            LazyColumn(
                Modifier::empty().size(Size {
                    width: 200.0,
                    height: 200.0,
                }),
                state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(1, |_index| {
                        Popup(
                            Rect {
                                x: 120.0,
                                y: 90.0,
                                width: 0.0,
                                height: 0.0,
                            },
                            Point { x: 0.0, y: 0.0 },
                            || {
                                Column(
                                    Modifier::empty()
                                        .size(Size {
                                            width: 15.0,
                                            height: 15.0,
                                        })
                                        .background(MARKER),
                                    ColumnSpec::default(),
                                    || {},
                                );
                            },
                        );
                    });
                },
            );
        });
    };

    composition.render(key, &mut content).expect("render");
    let scene = {
        let mut scene = None;
        for _ in 0..8 {
            settle(&mut composition, key, &mut content);
            let root = composition.root().expect("root");
            let layout = compute_layout(&mut composition, root);
            scene = Some(HeadlessRenderer::new().render(&layout));
        }
        scene.expect("scene")
    };

    let rects = marker_rects(&scene);
    assert_eq!(
        rects.len(),
        1,
        "a Popup composed inside a LazyColumn item must reach the enclosing \
         PopupHost, got {rects:?}"
    );
    assert!(
        (rects[0].x - 120.0).abs() <= 0.5 && (rects[0].y - 90.0).abs() <= 0.5,
        "overlay marker should paint at the anchor (120,90), was {:?}",
        rects[0]
    );
}

#[test]
fn popup_inside_lazy_column_item_is_removed_when_no_longer_composed() {
    use cranpose_core::mutableStateOf;

    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let show = mutableStateOf(true);
    let mut content = move || {
        PopupHost(move || {
            let state = rememberLazyListState();
            LazyColumn(
                Modifier::empty().size(Size {
                    width: 200.0,
                    height: 200.0,
                }),
                state,
                LazyColumnSpec::default(),
                move |scope| {
                    scope.items(1, move |_index| {
                        if show.value() {
                            Popup(
                                Rect {
                                    x: 120.0,
                                    y: 90.0,
                                    width: 0.0,
                                    height: 0.0,
                                },
                                Point { x: 0.0, y: 0.0 },
                                || {
                                    Column(
                                        Modifier::empty()
                                            .size(Size {
                                                width: 15.0,
                                                height: 15.0,
                                            })
                                            .background(MARKER),
                                        ColumnSpec::default(),
                                        || {},
                                    );
                                },
                            );
                        }
                    });
                },
            );
        });
    };

    let render_scene = |composition: &mut Composition<MemoryApplier>, content: &mut dyn FnMut()| {
        let mut scene = None;
        for _ in 0..8 {
            settle(composition, key, content);
            let root = composition.root().expect("root");
            let layout = compute_layout(composition, root);
            scene = Some(HeadlessRenderer::new().render(&layout));
        }
        scene.expect("scene")
    };

    composition.render(key, &mut content).expect("render");
    let scene = render_scene(&mut composition, &mut content);
    assert_eq!(
        marker_rects(&scene).len(),
        1,
        "popup visible while composed inside the lazy item"
    );

    show.set(false);
    let scene = render_scene(&mut composition, &mut content);
    assert_eq!(
        marker_rects(&scene).len(),
        0,
        "popup removed from the overlay after the lazy item stops composing it \
         (regression: selection handles persisted after the editor closed)"
    );
}
