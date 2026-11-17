// The "How it works" section: a little studio that shows one live
// paragraph twice. A green paper sweep lit through a window; a drawing board
// with the paragraph taped to it, the flat copy, and a glossy print of the
// same paragraph standing on its edge, the bent copy. The stage stays in
// view while the scroll walks four steps: the layout's boxes are ruled over
// the flat copy, the camera pulls back and the print comes up, the print
// curls, and a cord ties a spot on the print to the same spot on the board.
//
// Both copies are one composable drawn on two meshes: a word selected on
// one is selected on the other, and the last line can be edited on either.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{derivedStateOf, remember, rememberMutableStateOf};
use cranpose_ui::{
    composable,
    space::{Light, Multisample, Scene3d, Vec3},
    Box, BoxSpec, Color, Modifier, Point, ScrollState, Size,
};

use super::{
    cord::Cord,
    drawing_board::DrawingBoard,
    figure::{timeline, Figure, Plan},
    how_steps::HowSteps,
    paragraph::{LayoutNow, Paragraph},
    photo_print::PrintLabel,
    point_back::PointBack,
    sheet_layout::{SheetLayout, EDIT_LINE},
    sweep::Sweep,
    walk::{ease_softly, Walk},
};
use crate::app::ticking::Ticking;

// How many window heights the section takes: one page, then the walk.
pub(super) const HOW_PAGES: f32 = 2.6;
const DEEP_GREEN: Color = Color(0.03, 0.30, 0.16, 1.0);
// Where along the walk "flat copy" comes up on the board.
const FLAT_LABEL_IN: f32 = 0.38;
// The light through the window: toward the viewer's right, high and in
// front, in the scene's space, whose y runs down and z away.
const WINDOW_LIGHT: Light = Light {
    direction: Vec3::new(-0.4009, 0.6113, 0.6815),
    ambient: 0.55,
    shadows: true,
    size: 0.06,
};

// The section, `top` pixels down the article that `scroll` scrolls.
#[composable(no_skip)]
pub(super) fn HowSection(size: Size, scroll: ScrollState, top: f32, moving: bool) {
    let walk = Walk {
        scroll,
        top,
        travel: size.height * (HOW_PAGES - 1.0),
    };
    Box(
        Modifier::empty()
            .required_size(Size::new(size.width, size.height * HOW_PAGES))
            .test_tag("how"),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .required_size(size)
                    .graphics_layer_block(move |layer| layer.translation_y = walk.hold())
                    .clip_to_bounds()
                    .background(DEEP_GREEN),
                BoxSpec::default(),
                move || HowStage(size, walk, moving),
            );
        },
    );
}

#[composable(no_skip)]
fn HowStage(size: Size, walk: Walk, moving: bool) {
    let plan = Plan::new(size);
    let sway = rememberMutableStateOf(|| 0.0f32);
    let point = remember(PointBack::new).with(|point| *point);
    let layout: LayoutNow = remember(move || {
        Rc::new(RefCell::new(Rc::new(SheetLayout::new(
            plan.u,
            plan.sheet.height,
            EDIT_LINE,
        ))))
    })
    .with(Rc::clone);
    let pose = move || Figure::at(plan, walk.progress(), sway.get());
    let labelled = derivedStateOf(move || walk.progress() > FLAT_LABEL_IN).get();
    let bent = derivedStateOf(move || walk.progress() > timeline::BEND.0).get();
    let pointing = derivedStateOf(move || walk.progress() > 0.7).get();
    let ticking_layout = Rc::clone(&layout);
    Ticking(moving && (bent || pointing), move |seconds| {
        // The print sways while it is bent, and holds still under a
        // pointer, so its words can be picked.
        let scroll = walk.scroll.value_non_reactive();
        if walk.progress() > timeline::BEND.0 && point.pointer_at(scroll).is_none() {
            sway.set(sway.get_non_reactive() + seconds);
        }
        if walk.progress() > 0.7 {
            point.advance(seconds, &ticking_layout.borrow(), scroll);
        }
    });
    // The scene node's middle is where the camera's axis lands: the middle
    // of the sheet. It reaches past the stage on every side.
    let middle = Point::new(
        plan.sheet.x + plan.sheet.width * 0.5,
        plan.sheet.y + plan.sheet.height * 0.5,
    );
    let node = Size::new(
        2.0 * middle.x.max(size.width - middle.x),
        2.0 * middle.y.max(size.height - middle.y),
    );
    let paragraph_layout = Rc::clone(&layout);
    Box(
        Modifier::empty()
            .offset(middle.x - node.width * 0.5, middle.y - node.height * 0.5)
            .required_size(node)
            .scene3d(move || Scene3d {
                camera: pose().camera(node),
                light: Some(WINDOW_LIGHT),
                eye: None,
                multisample: Multisample::X4,
            }),
        BoxSpec::default(),
        move || {
            Sweep(node, plan, move || {
                ease_softly(
                    timeline::PULL_BACK.0,
                    timeline::PULL_BACK.1,
                    walk.progress(),
                )
            });
            DrawingBoard((node, plan), labelled, pose);
            Paragraph(
                (node, plan),
                (walk, pose),
                point,
                Rc::clone(&paragraph_layout),
            );
            if bent {
                PrintLabel(node, plan, pose);
            }
            if pointing {
                Cord(node, (walk, pose), point, Rc::clone(&paragraph_layout));
            }
        },
    );
    HowSteps(size, plan, walk);
}
