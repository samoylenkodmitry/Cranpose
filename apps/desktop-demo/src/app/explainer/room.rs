// The lamp room at this moment: a dark room seen from the front, its back
// wall a plate with the words cut through it and one lamp behind. The lamp
// swings in an arc as the scroll walks the section and leans toward the
// pointer; everything the room shows follows from where it is.

use cranpose_ui::{
    space::{Camera, Light, Mat4, Vec3},
    Point, Size,
};

use super::{
    hover::Hover,
    walk::{ease, Walk},
};

// Shares of the stage's height.
const FLOOR_LINE: f32 = 0.655;
const EYE_DISTANCE: f32 = 1.42;
const EYE_HEIGHT: f32 = 0.06;
const LAMP_DEPTH: f32 = 0.45;
const WORDS_HEIGHT: f32 = 0.25;
// Radians: shadows soften away from their casters.
const LAMP_SIZE: f32 = 0.03;

#[derive(Clone, Copy)]
pub(super) struct Room {
    pub walk: Walk,
    pub hover: Hover,
    pub size: Size,
    pub u: f32,
}

impl Room {
    pub(super) fn floor_line(self) -> f32 {
        (self.size.height * FLOOR_LINE).round()
    }

    pub(super) fn ahead(self) -> Point {
        Point::new(self.size.width * 0.5, self.size.height * EYE_HEIGHT)
    }

    pub(super) fn eye_distance(self) -> f32 {
        self.size.height * EYE_DISTANCE
    }

    pub(super) fn camera(self) -> Camera {
        let (width, height) = (self.size.width, self.size.height);
        let distance = self.eye_distance();
        let shear = (self.ahead().y - height * 0.5) / distance;
        Camera {
            view: Mat4::from_rows([
                [1.0, 0.0, 0.0, -width * 0.5],
                [0.0, 1.0, shear, -height * 0.5],
                [0.0, 0.0, 1.0, distance],
                [0.0, 0.0, 0.0, 1.0],
            ]),
            focal_length: distance,
        }
    }

    pub(super) fn lamp(self) -> Vec3 {
        let (width, height) = (self.size.width, self.size.height);
        let along = ((self.walk.progress() - 0.04) / 0.96).clamp(0.0, 1.0);
        let eased = along * along * (3.0 - 2.0 * along);
        let reach = 0.27 * if width < 760.0 { 0.7 } else { 1.0 };
        let half_turn = std::f32::consts::PI * eased;
        let (pointer, presence) = self.hover.now();
        let lean = Point::new(
            (pointer.x / width - 0.5) * width * 0.5 * presence,
            (pointer.y / height - 0.4) * height * 0.26 * presence,
        );
        Vec3::new(
            width * (0.5 - reach * half_turn.cos()) + lean.x,
            height * (-0.25 + 0.06 * half_turn.sin() - 0.02 * eased) + lean.y,
            height * LAMP_DEPTH,
        )
    }

    pub(super) fn light(self) -> Light {
        let lamp = self.lamp();
        let words = Vec3::new(
            self.size.width * 0.5 + (lamp.x - self.size.width * 0.5) * 0.3,
            self.size.height * WORDS_HEIGHT,
            0.0,
        );
        Light {
            direction: words - lamp,
            ambient: 0.04,
            shadows: true,
            size: LAMP_SIZE,
        }
    }

    pub(super) fn lamp_colour(self) -> [f32; 3] {
        let on = self.lamp_on();
        let strength = 0.13 + 0.87 * on * on * 0.4 + 0.87 * on * 0.6;
        let ember = [1.0, 0.46, 0.16];
        let warm = [1.07, 0.92, 0.76];
        std::array::from_fn(|channel| {
            (ember[channel] + (warm[channel] - ember[channel]) * on) * strength
        })
    }

    pub(super) fn lamp_on(self) -> f32 {
        ease(0.0, 0.15, self.walk.progress())
    }

    pub(super) fn lamp_image(self) -> Point {
        let lamp = self.lamp();
        let depth = self.size.height * LAMP_DEPTH;
        let toward_eye = depth / (depth + self.eye_distance());
        let ahead = self.ahead();
        Point::new(
            lamp.x + (ahead.x - lamp.x) * toward_eye,
            lamp.y + (ahead.y - lamp.y) * toward_eye,
        )
    }

    pub(super) fn hot(self) -> Point {
        let image = self.lamp_image();
        Point::new(
            image.x,
            self.size.height * 0.2 + (image.y + 0.2 * self.size.height) * 0.35,
        )
    }
}
