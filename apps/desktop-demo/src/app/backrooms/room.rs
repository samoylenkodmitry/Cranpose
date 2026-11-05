// A room of the backrooms and, behind its doorways, the rooms beyond them:
// each a scene behind a doorway of the room before, as deep as the eye
// sees, the rooms behind its own room's doorways and the rooms behind
// theirs. The walls, floors and ceilings show their finish alone; the
// entity's writing, a lever and the things in a room show live content.

use std::sync::Arc;

use cranpose_core::{key, rememberMutableStateOf, LaunchedEffect};
use cranpose_ui::{
    composable,
    space::{Camera, Light, Mat4, Material, Mesh, MeshSurface, Scene3d, Vec3},
    text::{FontWeight, TextAlign, TextUnit},
    Alignment, Box, BoxSpec, Color, Eye, Modifier, Rect, Size, Text, TextStyle,
};

use super::{
    anomaly::Anomaly,
    figure::Figure,
    finish::{Finish, Lamps, Sight},
    journey::Journey,
    notes,
    place::{spot, Place},
    plan::{Plan, Slot, Visit, Writing, DOOR_HEIGHT, DOOR_WIDTH, HEIGHT},
    printout::Printout,
};
use crate::app::controls_ui::{
    rememberControl, ControlKind, ControlModel, STAGE_HEIGHT, WALL_CARD_WIDTH,
};

// How many doorways deep the rooms open: the rooms that deep show the dark
// behind their doorways.
const MAX_DEPTH: u8 = 2;
// The content of a surface that shows its finish alone.
const BARE: Size = Size {
    width: 4.0,
    height: 4.0,
};
const DOOR: Size = Size {
    width: DOOR_WIDTH,
    height: DOOR_HEIGHT,
};
// A doorway's camera only places the room behind it; it shows nothing.
const DOORWAY_FOCAL_LENGTH: f32 = 500.0;
const DOWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);
// The writing on a wall: its content, its top above the floor and how far
// it stands off the wall.
const SCRAWL: Size = Size {
    width: 260.0,
    height: 150.0,
};
const SCRAWL_TOP: f32 = 228.0;
const SCRAWL_OUT: f32 = 0.8;
// A lever's card, as large on the wall as its content against a card on
// the controls tab, its top above the floor.
const LEVER_SCALE: f32 = 0.45;
const LEVER_TOP: f32 = 150.0;
// The light the lit things in a room take, from the lamps above.
pub(super) const LIGHT: Light = Light {
    direction: Vec3::new(0.19, 0.93, 0.32),
    ambient: 0.6,
    shadows: false,
    size: 0.0,
};

// The meshes every room's surfaces are drawn on, made once.
#[derive(Clone)]
pub(super) struct Meshes {
    pub(super) plane: Arc<Mesh>,
    pub(super) sphere: Arc<Mesh>,
    pub(super) cuboid: Arc<Mesh>,
}

impl Meshes {
    pub(super) fn new() -> Self {
        Self {
            plane: Arc::new(Mesh::plane()),
            sphere: Arc::new(Mesh::sphere(32, 16)),
            cuboid: Arc::new(Mesh::cuboid()),
        }
    }
}

// What every room shares: its meshes, the window's eye and the journey.
#[derive(Clone)]
pub(super) struct Level {
    pub(super) meshes: Meshes,
    pub(super) eye: Eye,
    pub(super) journey: Journey,
}

impl Level {
    fn sight(&self, place: Place) -> Sight {
        Sight {
            eye: self.eye.clone(),
            journey: self.journey,
            from_root: place.from_root,
        }
    }
}

// The room of `visit` at `place`, and the rooms behind its doorways. A room
// behind a doorway has no doorway at the door the eye would come in by: the
// room before it lies there.
#[composable(no_skip)]
pub(super) fn Room(level: Level, visit: Visit, place: Place) {
    let plan = Plan::of(visit.room);
    let size = plan.size();
    let lamps = Lamps::of(&plan);
    let sight = level.sight(place);
    let walls = plan
        .panels()
        .into_iter()
        .map(|panel| (Finish::Wallpaper, panel.transform(size), true));
    let pillars = plan
        .pillar_faces()
        .into_iter()
        .map(|face| (Finish::Wallpaper, face, true));
    let faces = walls
        .chain([
            (Finish::Carpet, floor(size), false),
            (Finish::Tiles, ceiling(size), false),
        ])
        .chain(pillars);
    for (index, (finish, transform, solid)) in faces.enumerate() {
        let (sight, plane) = (sight.clone(), Arc::clone(&level.meshes.plane));
        key(("face", index), move || {
            Face(sight, plane, (finish, lamps), transform, solid);
        });
    }
    let entry = visit.back.filter(|_| place.depth > 0).map(|back| back.door);
    for door in (0..plan.doors.len()).filter(|&door| Some(door) != entry) {
        let Some(frame) = plan.door_frame(door) else {
            continue;
        };
        let level = level.clone();
        let sight = sight.clone();
        key(("door", door), move || {
            if place.depth < MAX_DEPTH {
                Doorway(level, visit.through(door), frame, place);
            } else {
                let opening = Mat4::scaling(Vec3::new(DOOR_WIDTH, DOOR_HEIGHT, 1.0)).then(&frame);
                let plane = level.meshes.plane;
                Face(sight, plane, (Finish::Void, lamps), opening, false);
            }
        });
    }
    for (index, &writing) in plan.writings.iter().enumerate() {
        let (sight, plane) = (sight.clone(), Arc::clone(&level.meshes.plane));
        key(("writing", index), move || {
            Scrawl(sight, plane, lamps, writing, size);
        });
    }
    if let Some(thing) = plan.thing {
        Anomaly(level.clone(), sight.clone(), lamps, thing, plan.middle());
    }
    if let Some(slot) = plan.lever {
        Lever(level.clone(), slot, size);
    }
    if let Some(printout) = plan.printout {
        let (sight, plane) = (sight.clone(), Arc::clone(&level.meshes.plane));
        key("printout", move || {
            Printout(sight, plane, lamps, printout, size);
        });
    }
    if plan.figure && place.depth > 0 {
        Figure(
            sight,
            Arc::clone(&level.meshes.plane),
            lamps,
            plan.far_spot(),
        );
    }
}

// The floor of a room of `size`, its seen side up.
fn floor((width, depth): (f32, f32)) -> Mat4 {
    Mat4::from_axes(
        Vec3::new(width, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -depth),
        DOWN,
        Vec3::new(0.0, 0.0, depth),
    )
}

// The ceiling of a room of `size`, its seen side down.
fn ceiling((width, depth): (f32, f32)) -> Mat4 {
    Mat4::from_axes(
        Vec3::new(width, 0.0, 0.0),
        Vec3::new(0.0, 0.0, depth),
        -DOWN,
        Vec3::new(0.0, -HEIGHT, 0.0),
    )
}

// A flat part of a room that shows its finish alone: the unit plane placed
// by `transform`.
#[composable(no_skip)]
fn Face(
    sight: Sight,
    plane: Arc<Mesh>,
    (finish, lamps): (Finish, Lamps),
    transform: Mat4,
    solid: bool,
) {
    Box(
        spot(BARE)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform,
                material: sight.material(finish, &transform, lamps),
                casts_shadow: false,
                solid,
            })
            .background(finish.plain()),
        BoxSpec::default(),
        || {},
    );
}

// A doorway on `frame` into the room of `inner`, which lies behind it: a
// scene whose camera stands in the doorway the room behind is entered by.
#[composable(no_skip)]
fn Doorway(level: Level, inner: Visit, frame: Mat4, place: Place) {
    let plan = Plan::of(inner.room);
    let entry = inner.back.map_or(0, |back| back.door);
    let (Some(middle), Some(slot)) = (plan.door_middle(entry), plan.doors.get(entry)) else {
        return;
    };
    let camera = Camera::look_at(
        middle,
        middle + slot.side.inward(),
        DOWN,
        DOORWAY_FOCAL_LENGTH,
    );
    let opening = Rect {
        x: 0.0,
        y: 0.0,
        width: DOOR_WIDTH,
        height: DOOR_HEIGHT,
    };
    let behind = place.beyond(&camera.doorway(opening).then(&frame));
    Box(
        Modifier::empty().transform3d(move || frame),
        BoxSpec::default(),
        move || {
            let level = level.clone();
            // A screen reader reads the room the eye stands in, not the
            // rooms it sees through doorways, which are endless.
            Box(
                spot(DOOR)
                    .hide_from_accessibility()
                    .scene3d(move || Scene3d {
                        camera,
                        light: Some(LIGHT),
                        eye: None,
                    }),
                BoxSpec::default(),
                move || Room(level.clone(), inner, behind),
            );
        },
    );
}

// What the entity wrote on a wall, a little askew, in the room's light.
#[composable(no_skip)]
fn Scrawl(sight: Sight, plane: Arc<Mesh>, lamps: Lamps, writing: Writing, size: (f32, f32)) {
    let frame = writing
        .slot
        .frame((SCRAWL.width, SCRAWL_TOP), SCRAWL_OUT, size);
    let half = Vec3::new(SCRAWL.width * 0.5, SCRAWL.height * 0.5, 0.0);
    let slant = (writing.words % 7) as f32 * 0.025 - 0.075;
    let transform = Mat4::scaling(Vec3::new(SCRAWL.width, SCRAWL.height, 1.0))
        .then(&Mat4::translation(-half))
        .then(&Mat4::rotation_z(slant))
        .then(&Mat4::translation(half))
        .then(&frame);
    let journey = sight.journey;
    let words = writing.words;
    Box(
        spot(SCRAWL).mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&plane),
            transform,
            material: sight.material(Finish::Content, &transform, lamps),
            casts_shadow: false,
            solid: false,
        }),
        BoxSpec {
            content_alignment: Alignment::CENTER,
            ..BoxSpec::default()
        },
        move || {
            Text(
                notes::wall(words, &journey),
                Modifier::empty().fill_max_width(),
                scrawl_style(),
            );
        },
    );
}

fn scrawl_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(27.0);
    style.span_style.font_weight = Some(FontWeight::BOLD);
    style.span_style.color = Some(Color(0.30, 0.05, 0.03, 0.9));
    style.paragraph_style.text_align = TextAlign::Center;
    style
}

// A lever on a wall that puts out the lights of every room, or brings them
// back.
#[composable(no_skip)]
fn Lever(level: Level, slot: Slot, size: (f32, f32)) {
    let control = rememberControl(ControlKind::LeverSwitch);
    let on = control.is_on();
    let last = rememberMutableStateOf(move || on);
    let journey = level.journey;
    LaunchedEffect(on, move |_| {
        if last.get_non_reactive() != on {
            last.set(on);
            journey.pull();
        }
    });
    let card = Size {
        width: WALL_CARD_WIDTH,
        height: STAGE_HEIGHT,
    };
    let frame = Mat4::scaling(Vec3::new(LEVER_SCALE, LEVER_SCALE, LEVER_SCALE)).then(&slot.frame(
        (card.width * LEVER_SCALE, LEVER_TOP),
        1.0,
        size,
    ));
    let plane = Arc::clone(&level.meshes.plane);
    Box(
        spot(card).mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&plane),
            transform: Mat4::scaling(Vec3::new(card.width, card.height, 1.0)).then(&frame),
            material: Material::Lit,
            casts_shadow: false,
            solid: true,
        }),
        BoxSpec::default(),
        move || {
            ControlModel(
                ControlKind::LeverSwitch,
                control.clone(),
                Modifier::empty(),
                card,
            );
        },
    );
}
