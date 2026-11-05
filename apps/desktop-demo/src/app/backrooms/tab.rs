// The Backrooms tab: a camcorder's view of yellow rooms under humming
// lamps that never end. The eye's room is the window's scene; the rooms
// behind its doorways, and behind theirs, are scenes behind doorways, and
// the walk through a doorway makes the room behind it the window's.

use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode, StartOffset,
};
use cranpose_core::{
    derivedStateOf, key, remember, CompositionLocalProvider, LaunchedEffect, SideEffect, State,
};
use cranpose_ui::{
    composable, local_eye, space::Camera, Box, BoxSpec, BoxWithConstraints,
    BoxWithConstraintsScope, Color, Modifier,
};

use super::{
    hud::Hud,
    journey::rememberJourney,
    level::{local_level, Level, Meshes},
    place::Place,
    room::{Room, LIGHT},
    shelf::{rememberShelf, Printer},
    walker::{rememberWalker, Walker},
};
use crate::app::ticking::Ticking;

// The camera's vertical field of view, in radians: a camcorder's wide end.
const FIELD_OF_VIEW: f32 = 1.2;
// The camcorder's frames a second, at which the lamps flicker and the grain
// moves, and how many it runs before its film starts over: ten minutes.
const FRAMES_PER_SECOND: u32 = 24;
const REEL_FRAMES: u32 = FRAMES_PER_SECOND * 600;

#[composable]
pub(crate) fn BackroomsTab() {
    let journey = rememberJourney();
    let walker = rememberWalker(journey);
    let shelf = rememberShelf();
    let level = Level {
        meshes: remember(Meshes::new).with(Clone::clone),
        journey,
        film: rememberFilm(),
        shelf,
    };
    Printer(shelf);
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
            // Every room reads the level and the window's eye from here.
            let (level, walker) = (level.clone(), walker.clone());
            let eye = walker.eye.clone();
            CompositionLocalProvider(
                vec![
                    local_level().provides(Some(level)),
                    local_eye().provides(Some(eye)),
                ],
                move || Rooms(walker.clone(), focal_length),
            );
            Hud(journey);
        },
    );
}

// The camcorder's frame, counted from the frame clock by an infinite
// transition. The transition's value changes each display frame; the frame
// it derives changes 24 times a second, and only then are its readers told.
#[composable]
fn rememberFilm() -> State<u32> {
    let reel = rememberInfiniteTransition("camcorder_film").animateFloat(
        0.0,
        REEL_FRAMES as f32,
        infiniteRepeatable(
            AnimationSpec::linear(u64::from(REEL_FRAMES) * 1000 / u64::from(FRAMES_PER_SECOND)),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "camcorder_film",
    );
    derivedStateOf(move || reel.value() as u32)
}

// Walks the eye each frame while a key or a finger walks it.
#[composable]
fn Walking(walker: Walker) {
    let stepper = walker.clone();
    Ticking(walker.is_moving(), move |seconds| stepper.step(seconds));
}

// The window's scene through the eye, which holds the eye's room. A new
// room is a new scene, so the eye knows when the renderer has placed it.
#[composable]
fn Rooms(walker: Walker, focal_length: f32) {
    let visit = walker.journey.visit();
    key(visit, move || {
        let sees = walker.clone();
        let scene = Box(
            Modifier::empty()
                .fill_max_size()
                .scene3d(move || sees.scene(focal_length, Some(LIGHT))),
            BoxSpec::default(),
            move || Room(visit, Place::ROOT),
        );
        // The walker learns the scene's node once the composition applies.
        SideEffect(move || walker.scene.set(Some(scene)));
    });
}
