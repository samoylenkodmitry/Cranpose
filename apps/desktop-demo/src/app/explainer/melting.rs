// The melt at this moment: how far the scroll has walked the section and
// where the pointer warms it, read where each part is drawn. Everything
// else follows from these two.

use cranpose_ui::{Point, Size};

use super::{
    hover::Hover,
    sag::{melt_at, Sag, Warmth},
    walk::{ease, Walk},
};

#[derive(Clone, Copy)]
pub(super) struct Melting {
    pub walk: Walk,
    pub hover: Hover,
    pub size: Size,
    pub u: f32,
    pub split: f32,
}

impl Melting {
    pub(super) fn progress(self) -> f32 {
        self.walk.progress()
    }

    pub(super) fn sag(self) -> Sag {
        Sag {
            warmth: self.warmth(),
            ..self.cold_sag()
        }
    }

    fn cold_sag(self) -> Sag {
        Sag {
            melt: melt_at(self.progress()),
            warmth: Warmth::default(),
            u: self.u,
            split: self.split,
        }
    }

    pub(super) fn warmth(self) -> Warmth {
        let (at, strength) = self.hover.now();
        if strength <= 0.0 {
            return Warmth::default();
        }
        let sag = self.cold_sag();
        let y = (0..3).fold(at.y, |y, _| at.y - sag.at(Point::new(at.x, y)));
        Warmth {
            strength,
            at: Point::new(at.x, y),
        }
    }

    pub(super) fn softness(self) -> f32 {
        ease(0.03, 0.45, self.progress()) * 0.92
    }

    pub(super) fn swelling(self) -> f32 {
        let progress = self.progress();
        ease(0.02, 0.3, progress) * 0.55 + ease(0.2, 1.0, progress) * 0.5
    }

    pub(super) fn heat(self) -> f32 {
        ease(0.1, 1.0, self.progress()) * 0.8
    }
}
