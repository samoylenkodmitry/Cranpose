// The Backrooms tab: a camcorder's view of an endless maze of yellow rooms
// under humming lamps, walked the way the Room tab walks.

use std::{sync::Arc, time::Duration};

use cranpose_core::{
    delay, remember, rememberMutableStateOf, LaunchedEffect, LaunchedEffectAsync, MutableState,
};
use cranpose_ui::{
    composable,
    space::{Camera, Scene3d},
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Color, Modifier, Size,
};

use super::{
    figure::Figure,
    hud::Hud,
    level::{self, Frame},
    walker::{rememberWalker, Walker},
};
use crate::app::ticking::Ticking;

// The camera's vertical field of view, in radians: a camcorder's wide end.
const FIELD_OF_VIEW: f32 = 1.2;
// The camcorder's frames a second, at which the lamps flicker and the grain
// moves.
pub(super) const FRAMES_PER_SECOND: u32 = 24;
const CAMCORDER_FRAME: Duration = Duration::from_micros(41_667);
// The level draws nothing from its surface's content, so the content is
// as small as content gets.
const CONTENT: Size = Size {
    width: 4.0,
    height: 4.0,
};

#[composable]
pub(crate) fn BackroomsTab() {
    let walker = rememberWalker();
    let frames = rememberMutableStateOf(|| 0_u32);
    let figure = rememberMutableStateOf(Figure::default);
    Camcorder(walker.clone(), frames, figure);
    Walking(walker.clone());
    let focus = walker.focus.clone();
    LaunchedEffect((), move |_| {
        if let Err(error) = focus.request_focus() {
            log::warn!("the backrooms could not take focus: {error:?}");
        }
    });
    let (keys, scroll, look) = (walker.clone(), walker.clone(), walker.clone());
    BoxWithConstraints(
        Modifier::empty()
            .fill_max_size()
            .background(Color::BLACK)
            .focus_requester(&walker.focus)
            .focus_target()
            .on_key_event(move |event| keys.key(event))
            .on_rotary_scroll_event(move |event| scroll.scroll(event))
            .pointer_input((), move |scope| {
                let look = look.clone();
                async move { look.look(scope).await }
            }),
        move |scope| {
            let height = scope.constraints().max_height;
            let height = if height.is_finite() { height } else { 1.0 };
            let focal_length = Camera::focal_length_for(height.max(1.0), FIELD_OF_VIEW);
            Level(walker.clone(), focal_length, (frames, figure));
            Hud(frames);
        },
    );
}

// The camcorder's clock, which counts its frames, and the figure, which it
// moves along.
#[composable(no_skip)]
fn Camcorder(walker: Walker, frames: MutableState<u32>, figure: MutableState<Figure>) {
    LaunchedEffectAsync((), move |scope| {
        Box::pin(async move {
            let step = CAMCORDER_FRAME.as_secs_f32();
            while scope.is_active() {
                delay(CAMCORDER_FRAME).await;
                frames.update(|count| *count = count.wrapping_add(1));
                let (eye, cell) = (walker.pose(), walker.cell());
                figure.update(|figure| figure.tick(step, &eye, cell));
            }
        })
    });
}

// Walks the eye each frame while a key or a finger walks it.
#[composable(no_skip)]
fn Walking(walker: Walker) {
    let stepper = walker.clone();
    Ticking(walker.is_moving(), move |seconds| stepper.step(seconds));
}

// The scene through the eye, and the level in it.
#[composable(no_skip)]
fn Level(
    walker: Walker,
    focal_length: f32,
    (frames, figure): (MutableState<u32>, MutableState<Figure>),
) {
    let view_box = remember(|| level::view_box().map(Arc::new)).with(Clone::clone);
    let sees = walker.clone();
    Box(
        Modifier::empty().fill_max_size().scene3d(move || Scene3d {
            camera: sees.camera(focal_length),
            light: None,
            eye: None,
        }),
        BoxSpec::default(),
        move || {
            let Some(view_box) = view_box.clone() else {
                return;
            };
            let shows = walker.clone();
            Box(
                Modifier::empty()
                    .required_size(CONTENT)
                    .mesh_surface(move || {
                        let (figure, cell) = (figure.get(), shows.cell());
                        Frame {
                            frame: frames.get(),
                            cell,
                            dim: figure.dim(),
                            figure: figure.at(cell),
                        }
                        .surface(&view_box)
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}
