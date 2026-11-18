// One postage stamp of the demo gallery: a picture of a demo tab and the
// tab's name, printed on perforated paper lying in its sheet. Under the
// pointer it tears free along its perforations and curls up from its far
// corner; a click opens its tab. Its name stays live text, which selects
// where it shows on the curled paper.

use std::{
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_animation::{spring, Animatable};
use cranpose_core::{remember, with_current_composer, CompositionLocalProvider, MutableState};
use cranpose_ui::{
    composable, local_text_selection_colors,
    space::{Mat4, Material, MeshSurface, Shaded, Vec3},
    widgets::SelectionContainer,
    Box, BoxSpec, Brush, Canvas, Color, DrawStyle, Modifier, Path, Point, PointerEventKind,
    PointerInputScope, Rect, Size, Text, TextSelectionColors,
};

use super::{
    curl::{Bend, CurlMeshes, PEEL},
    lettering::strong,
    perforation::{perforated, Winding},
    stamp_art::StampArt,
};
use crate::app::DemoTab;

// The stamp paper as the content shows it; the shader lays its tooth on
// it.
pub(super) const CREAM: Color = Color(0.955, 0.92, 0.85, 1.0);
// The printing ink.
pub(super) const INK: Color = Color(0.09, 0.06, 0.16, 1.0);
// The colour the content marks a selection with: the paper shows it as a
// postmark.
pub(super) const MARKER: Color = Color(0.0, 1.0, 0.0, 1.0);
const PAPER_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl", "stamp_paper.wgsl");
// How deep the stamps lie in front of the ground.
const STAMP_DEPTH: f32 = -7.0;
// How far a stamp lifts while pressed, and under the pointer.
const PRESSED_LIFT: f32 = 0.3;
const HOVER_LIFT: f32 = 1.0;
// A stamp lifted further than this casts a shadow.
const SHADOW_LIFT: f32 = 0.3;

// How a stamp is laid out, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StampLayout {
    pub size: Size,
    // The picture's box on the stamp.
    pub art: Rect,
    // The name's size.
    pub name_size: f32,
    // The radius of a punched hole and how many run along the width and
    // down the height.
    pub hole: f32,
    pub holes: (usize, usize),
}

// What makes a stamp lift: its place in the sheet's arrival, read when it
// is drawn, and the lift it keeps at rest.
pub(super) struct Lift {
    pub resting: f32,
    pub arriving: Rc<dyn Fn() -> f32>,
}

// The stamp of `tab`, its top left corner `at`, curled by the meshes of
// `curls`. A click shows the tab in `open`.
#[composable(no_skip)]
pub(super) fn Stamp(
    tab: DemoTab,
    (at, layout): (Point, StampLayout),
    (curls, lift): (Rc<CurlMeshes>, Rc<Lift>),
    open: Option<MutableState<DemoTab>>,
) {
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let resting = lift.resting;
    let spring_lift = remember(move || Animatable::new(resting, runtime)).with(Clone::clone);
    let hover = spring_lift.clone();
    let size = layout.size;
    Box(
        Modifier::empty()
            .offset(at.x, at.y)
            .required_size(size)
            .pointer_input(tab.slug(), move |scope: PointerInputScope| {
                let lift = hover.clone();
                async move {
                    scope
                        .await_pointer_event_scope(|events| async move {
                            loop {
                                let event = events.await_pointer_event().await;
                                let target = match event.kind {
                                    PointerEventKind::Enter | PointerEventKind::Up => HOVER_LIFT,
                                    PointerEventKind::Down => PRESSED_LIFT,
                                    PointerEventKind::Exit | PointerEventKind::Cancel => resting,
                                    _ => continue,
                                };
                                lift.clone().animateTo(target, spring(0.74, 300.0));
                            }
                        })
                        .await;
                }
            })
            .clickable(move |_| {
                if let Some(open) = open {
                    open.set(tab);
                }
            })
            // The tab bar names the tab already; a reader tells the stamp
            // apart from it.
            .content_description(format!("{} stamp", tab.label())),
        BoxSpec::default(),
        move || {
            let (curls, lift, spring_lift) =
                (Rc::clone(&curls), Rc::clone(&lift), spring_lift.clone());
            Box(
                Modifier::empty().required_size(size).mesh_surface(move || {
                    let now = spring_lift.state().get().max((lift.arriving)());
                    let drawn = CurlMeshes::lift(CurlMeshes::level(now));
                    MeshSurface {
                        mesh: curls.mesh(drawn),
                        transform: Mat4::translation(Vec3::new(0.0, 0.0, STAMP_DEPTH)),
                        material: stamp_paper(
                            size,
                            Bend::at(drawn, size),
                            Some(layout.art),
                            layout.name_size,
                        ),
                        // Paper lying flat, or nearly, would only shadow
                        // itself.
                        casts_shadow: drawn > SHADOW_LIFT,
                        solid: true,
                    }
                }),
                BoxSpec::default(),
                move || StampFace(tab, layout),
            );
        },
    );
}

// The stamp's printed face: perforated paper, the picture and the name.
#[composable(no_skip)]
fn StampFace(tab: DemoTab, layout: StampLayout) {
    let size = layout.size;
    Canvas(Modifier::empty().required_size(size), move |scope| {
        let mut paper = Path::new();
        let edge = Rect {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        };
        perforated(
            &mut paper,
            edge,
            layout.holes,
            layout.hole,
            Winding::Clockwise,
        );
        scope.draw_path(&paper, Brush::solid(CREAM), DrawStyle::Fill);
    });
    let art = layout.art;
    StampArt(
        tab,
        Modifier::empty()
            .offset(art.x, art.y)
            .required_size(Size::new(art.width, art.height)),
    );
    let marker = TextSelectionColors {
        handle_color: MARKER,
        background_color: MARKER,
    };
    CompositionLocalProvider(
        [local_text_selection_colors().provides(marker)],
        move || {
            SelectionContainer(
                Modifier::empty().offset(art.x, art.y + art.height + layout.name_size * 0.3),
                move || {
                    Text(
                        tab.label(),
                        Modifier::empty(),
                        strong(layout.name_size, INK),
                    );
                },
            );
        },
    );
}

// Paper `size` big bent as `bend` says, with a picture in `art` and printed
// lines `line` tall.
pub(super) fn stamp_paper(size: Size, bend: Bend, art: Option<Rect>, line: f32) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let mut params = [0.0; Shaded::PARAMS];
    params[..7].copy_from_slice(&[
        size.width,
        size.height,
        bend.attached,
        bend.turn,
        bend.diagonal,
        PEEL[0],
        PEEL[1],
    ]);
    if let Some(art) = art {
        params[8..12].copy_from_slice(&[art.x, art.y, art.width, art.height]);
    }
    for (first, color) in [(12, CREAM), (16, INK), (20, MARKER)] {
        params[first..first + 3].copy_from_slice(&[color.0, color.1, color.2]);
    }
    params[24] = line;
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(PAPER_WGSL))),
        params,
    })
}
