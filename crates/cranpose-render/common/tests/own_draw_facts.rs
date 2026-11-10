// A record's own draw facts follow its draws as updates change them: an
// image among them makes them pixel-sensitive, and only shadows and
// content slots leave them undrawn.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, NodeId};
use cranpose_render_common::{
    graph_scene::Scene,
    scene_store::{ItemRef, LayerRef, OwnDrawFacts},
};
use cranpose_ui::{Brush, Canvas, Color, Modifier, Size, TestComposition, run_test_composition};
use cranpose_ui_graphics::{DrawPrimitive, ImageBitmap};

use crate::scene_probe::{initial_graph, update_scene};

const VIEWPORT: Size = Size::new(120.0, 120.0);

// A canvas that paints a rect, an image as well while `image` holds.
fn canvas(image: Rc<RefCell<Option<MutableState<bool>>>>) -> TestComposition {
    run_test_composition(move || {
        let shown = cranpose_core::rememberMutableStateOf(|| false);
        *image.borrow_mut() = Some(shown);
        Canvas(
            Modifier::empty().size(Size::new(40.0, 24.0)),
            move |scope| {
                scope.draw_rect_at(
                    cranpose_ui_graphics::Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 40.0,
                        height: 24.0,
                    },
                    Brush::solid(Color::RED),
                );
                if shown.get() {
                    scope.draw_image(
                        ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("an image"),
                    );
                }
            },
        );
    })
}

// The facts of the record that paints the canvas's rect.
fn canvas_facts(scene: &Scene) -> OwnDrawFacts {
    fn find(layer: LayerRef<'_>) -> Option<LayerRef<'_>> {
        let paints = layer.own_draws().any(|item| match item {
            ItemRef::Draw(draw) => matches!(draw.primitive, DrawPrimitive::Rect { .. }),
            ItemRef::Run(run) => run
                .primitives()
                .any(|primitive| matches!(primitive, DrawPrimitive::Rect { .. })),
            _ => false,
        });
        if paints {
            return Some(layer);
        }
        layer.children().find_map(find)
    }
    find(scene.root_layer().expect("a root"))
        .expect("a record paints the rect")
        .own_draw_facts()
}

#[test]
fn own_draw_facts_follow_an_image_drawn_and_dropped() {
    let image = Rc::default();
    let mut composition = canvas(Rc::clone(&image));
    let root: NodeId = composition.root().expect("a root");
    let mut scene = initial_graph(&mut composition, root, VIEWPORT);
    let rect_only = OwnDrawFacts {
        pixel_sensitive: false,
        drawn: true,
    };
    assert_eq!(
        canvas_facts(&scene),
        rect_only,
        "a rect is drawn, not pixel-sensitive"
    );

    image.borrow().expect("the image state").set(true);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);
    assert_eq!(
        canvas_facts(&scene),
        OwnDrawFacts {
            pixel_sensitive: true,
            drawn: true,
        },
        "an image makes the draws pixel-sensitive"
    );

    image.borrow().expect("the image state").set(false);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);
    assert_eq!(
        canvas_facts(&scene),
        rect_only,
        "the facts drop the image again"
    );
}
