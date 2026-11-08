// A scene's reach holds everything its records paint, and follows them as
// updates move them or change what they draw.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, NodeId};
use cranpose_render_common::{
    graph::ProjectiveTransform,
    graph_scene::Scene,
    scene_store::{ItemRef, LayerRef, Reach},
};
use cranpose_ui::{
    Box, BoxSpec, Brush, Canvas, Color, Column, ColumnSpec, GraphicsLayer, Modifier, Size,
    TestComposition, run_test_composition,
};
use cranpose_ui_graphics::{DrawPrimitive, Rect};

use crate::scene_probe::{initial_graph, update_scene};

const VIEWPORT: Size = Size::new(240.0, 240.0);

// A column of a spacer `height` tall above a canvas that paints `overhang`
// past each side of its 40 by 24 box.
fn spaced_canvas(
    height: Rc<RefCell<Option<MutableState<f32>>>>,
    overhang: Rc<RefCell<Option<MutableState<f32>>>>,
) -> TestComposition {
    run_test_composition(move || {
        let height = Rc::clone(&height);
        let overhang = Rc::clone(&overhang);
        Column(
            Modifier::empty().size(Size::new(200.0, 200.0)),
            ColumnSpec::default(),
            move || {
                let spacer = cranpose_core::rememberMutableStateOf(|| 20.0_f32);
                let past = cranpose_core::rememberMutableStateOf(|| 6.0_f32);
                *height.borrow_mut() = Some(spacer);
                *overhang.borrow_mut() = Some(past);
                Box(
                    Modifier::empty().size(Size::new(40.0, spacer.get())),
                    BoxSpec::default(),
                    || {},
                );
                Canvas(
                    Modifier::empty().size(Size::new(40.0, 24.0)),
                    move |scope| {
                        let past = past.get();
                        scope.draw_rect_at(
                            Rect {
                                x: -past,
                                y: -past,
                                width: 40.0 + 2.0 * past,
                                height: 24.0 + 2.0 * past,
                            },
                            Brush::solid(Color::RED),
                        );
                    },
                );
            },
        );
    })
}

// The rects `layer` and the records beneath it paint, in the space
// `to_root` maps `layer`'s into.
fn painted(layer: LayerRef<'_>, to_root: ProjectiveTransform, out: &mut Vec<Rect>) {
    for item in layer.items() {
        match item {
            ItemRef::Layer(child) | ItemRef::Leaf(child) => {
                painted(child, child.transform_to_parent().then(to_root), out);
            }
            ItemRef::Run(run) => {
                out.extend(run.primitives().filter_map(|primitive| match primitive {
                    DrawPrimitive::Rect { rect, .. } => Some(to_root.bounds_for_rect(rect)),
                    _ => None,
                }));
            }
            ItemRef::Draw(draw) => {
                if let DrawPrimitive::Rect { rect, .. } = &draw.primitive {
                    out.push(to_root.bounds_for_rect(*rect));
                }
            }
            ItemRef::Text(_) => {}
        }
    }
}

// The root's reach, checked to hold every rect the scene paints and to
// reach no further than they do.
fn checked_reach(scene: &mut Scene) -> Rect {
    assert_eq!(
        scene.root_layer().map(LayerRef::reach),
        Some(Reach::Unknown),
        "a changed scene's reach waits to be worked out"
    );
    scene.refresh_reach();
    let root = scene.root_layer().expect("a root");
    let mut rects = Vec::new();
    painted(root, ProjectiveTransform::identity(), &mut rects);
    let Reach::Finite(reach) = root.reach() else {
        panic!(
            "a scene of rects reaches a known rect, not {:?}",
            root.reach()
        );
    };
    let union = rects
        .iter()
        .copied()
        .reduce(|a, b| a.union(b))
        .expect("the scene paints");
    assert_eq!(reach, union, "the reach is the rects the scene paints");
    reach
}

#[test]
fn a_scene_reaches_where_its_records_paint_as_updates_move_them() {
    let (height, overhang) = (Rc::default(), Rc::default());
    let mut composition = spaced_canvas(Rc::clone(&height), Rc::clone(&overhang));
    let root: NodeId = composition.root().expect("a root");
    let mut scene = initial_graph(&mut composition, root, VIEWPORT);
    let first = checked_reach(&mut scene);
    assert_eq!(
        (first.x, first.y),
        (-6.0, 14.0),
        "the overhang reaches past"
    );

    height.borrow().expect("the spacer's height").set(120.0);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);
    let moved = checked_reach(&mut scene);
    assert_eq!(moved.y, 114.0, "the canvas moved down");

    overhang.borrow().expect("the overhang").set(1.0);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);
    let redrawn = checked_reach(&mut scene);
    assert_eq!((redrawn.x, redrawn.y), (-1.0, 119.0), "the reach shrank");
}

// A column, clipped when `clip` holds, around a box that casts a shadow.
fn shadowed(clip: bool) -> TestComposition {
    run_test_composition(move || {
        let column = Modifier::empty().size(Size::new(200.0, 200.0));
        Column(
            if clip {
                column.clip_to_bounds()
            } else {
                column
            },
            ColumnSpec::default(),
            || {
                Box(
                    Modifier::empty()
                        .size(Size::new(40.0, 40.0))
                        .graphics_layer(|| GraphicsLayer {
                            shadow_elevation: 8.0,
                            ..Default::default()
                        }),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    })
}

#[test]
fn a_shadow_leaves_the_reach_unknown_until_a_clip_bounds_it() {
    for clip in [false, true] {
        let mut composition = shadowed(clip);
        let root = composition.root().expect("a root");
        let mut scene = initial_graph(&mut composition, root, VIEWPORT);
        scene.refresh_reach();
        let root = scene.root_layer().expect("a root layer");
        let expected = if clip {
            Reach::Finite(Rect::from_size(Size::new(200.0, 200.0)))
        } else {
            Reach::Unknown
        };
        assert_eq!(root.reach(), expected, "clipped: {clip}");
        let caster = std::iter::successors(Some(root), |layer| layer.children().next())
            .find_map(|layer| layer.extent().caster)
            .expect("the box casts a shadow");
        assert_eq!(caster.height, 8.0);
        assert_eq!(
            (caster.bounds.width, caster.bounds.height),
            (40.0, 40.0),
            "the caster is the box"
        );
    }
}
