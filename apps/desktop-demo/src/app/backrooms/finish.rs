// The finish of the backrooms' surfaces: the shader of finish.wgsl, which
// every wall, floor and ceiling of every room shares with the things in
// them, and the values one surface sets for it.

use std::sync::{Arc, OnceLock};

use cranpose_ui::{
    space::{Mat4, Material, Shaded, Vec3},
    Color, Eye,
};

use super::{
    level::Level,
    place::Place,
    plan::{Mood, Plan, HEIGHT, UNIT},
};

// What a surface shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Finish {
    Wallpaper,
    Carpet,
    Tiles,
    // The surface's content, in the room's light.
    Content,
    // The dim far rooms behind a doorway too deep to open.
    Void,
}

impl Finish {
    // The content's colour, which the surface shows until its shader builds.
    pub(super) fn plain(self) -> Color {
        match self {
            Self::Wallpaper => Color(0.62, 0.55, 0.30, 1.0),
            Self::Carpet => Color(0.42, 0.36, 0.20, 1.0),
            Self::Tiles => Color(0.55, 0.53, 0.45, 1.0),
            Self::Content => Color::TRANSPARENT,
            Self::Void => Color(0.30, 0.26, 0.13, 1.0),
        }
    }

    fn param(self) -> f32 {
        match self {
            Self::Wallpaper => 0.0,
            Self::Carpet => 1.0,
            Self::Tiles => 2.0,
            Self::Content => 3.0,
            Self::Void => 4.0,
        }
    }
}

// The lamps of a room, as its surfaces read them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Lamps {
    size: (f32, f32),
    seed: u32,
    mood: Mood,
}

impl Lamps {
    pub(super) fn of(plan: &Plan) -> Self {
        Self {
            size: plan.size(),
            seed: plan.id.lamp_seed(),
            mood: plan.mood,
        }
    }
}

// How a room's surfaces are seen and lit, in the level all rooms share:
// the window's eye sees them
// through the doorways between the window scene's room and theirs, whose
// map `from_root` takes the window scene's coordinates into the room's;
// the room's lamps light them at the lights' power, and the grain moves
// with the camcorder's film.
#[derive(Clone)]
pub(super) struct Sight {
    pub(super) level: Level,
    pub(super) eye: Eye,
    pub(super) from_root: Mat4,
    lamps: Lamps,
}

impl Sight {
    pub(super) fn of(level: Level, eye: Eye, place: Place, lamps: Lamps) -> Self {
        Self {
            level,
            eye,
            from_root: place.from_root,
            lamps,
        }
    }

    // Where the eye stands, in the room. Read in a draw closure, it follows
    // the eye.
    pub(super) fn eye_in_room(&self) -> Vec3 {
        self.from_root.transform_point(self.eye.pose().position)
    }

    // The finish of a mesh that `placement` places in the room. Read in a
    // draw closure, it follows the camcorder's film and the lights' power;
    // the shader reads the eye from its surface.
    pub(super) fn material(&self, finish: Finish, placement: &Mat4) -> Material {
        let mut params = [0.0; Shaded::PARAMS];
        params[0] = finish.param();
        let axes = [
            (1, placement.transform_point(Vec3::ZERO)),
            (4, placement.transform_vector(Vec3::new(1.0, 0.0, 0.0))),
            (7, placement.transform_vector(Vec3::new(0.0, 1.0, 0.0))),
            (10, placement.transform_vector(Vec3::new(0.0, 0.0, 1.0))),
        ];
        for (first, value) in axes {
            params[first..first + 3].copy_from_slice(&value.to_array());
        }
        // The film wraps long before 2^24, past which an f32 skips frames.
        params[13] = self.level.film.get() as f32;
        params[14] = self.lamps.size.0;
        params[15] = self.lamps.size.1;
        params[16] = self.lamps.seed as f32;
        params[17] = match self.lamps.mood {
            Mood::Lit => 0.0,
            Mood::Dim => 1.0,
            Mood::Dark => 2.0,
        };
        params[18] = self.level.journey.power();
        Material::Shaded(Shaded {
            shader: shader(),
            params,
        })
    }
}

fn shader() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(SOURCE.get_or_init(|| {
        Arc::from(format!(
            "const UNIT: f32 = {UNIT:?};\nconst HEIGHT: f32 = {HEIGHT:?};\n{}",
            include_str!("finish.wgsl")
        ))
    }))
}
