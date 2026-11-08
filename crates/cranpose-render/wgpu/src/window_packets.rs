// The window packets a producer built last: what each changed, the one the
// next changes against, and the oldest one the window's targets hold, which
// a window walk culls against.

use cranpose_render_common::scene_store::SceneVersion;

use crate::frame::{Damage, DamageRects, FrameDamages, PacketDamage};

// What a packet draws: two packets of the same draw the same frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct PacketDraws {
    pub(crate) scene: SceneVersion,
    pub(crate) overlay: SceneVersion,
    pub(crate) viewport: (u32, u32),
    pub(crate) root_scale: u32,
    pub(crate) transparent: bool,
}

#[derive(Default)]
pub(crate) struct WindowPackets {
    damages: FrameDamages,
    // The window packet built last, which the next one changes against, and
    // what it drew: a capture of the same draws the same frame.
    last: Option<(u64, PacketDraws)>,
    // The damage of the window packet built last when it went back unshown:
    // the next window packet changes against what it changed against.
    carried: Option<PacketDamage>,
    held: Option<u64>,
}

impl WindowPackets {
    // The window packet built last.
    pub(crate) fn last(&self) -> Option<u64> {
        self.last.map(|(frame, _)| frame)
    }

    // The damages since the oldest packet the window's targets hold, and
    // the rects of `begun`: a region a window walk may cull to. `None` when
    // one of the damages is not a part of the window.
    pub(crate) fn cull_region(&self, begun: [DamageRects; 2]) -> Option<DamageRects> {
        let mut region = self.damages.union_back(self.last()?, self.held?).ok()?;
        for rect in begun.iter().flat_map(DamageRects::as_slice) {
            region.add(*rect);
        }
        Some(region)
    }

    // The damage of the next window packet, whose frame changes `damage` in
    // a window of `viewport` against `since`: what an unshown packet changed
    // against, when one went back.
    pub(crate) fn next_damage(
        &self,
        damage: Damage,
        since: Option<u64>,
        viewport: (u32, u32),
    ) -> PacketDamage {
        match self.carried {
            Some(carried) => PacketDamage {
                since: carried.since,
                damage: carried.damage.union(damage, viewport),
            },
            None => PacketDamage { since, damage },
        }
    }

    // What an image that holds the oldest frame the window's targets hold
    // repaints for `packet`, the next window packet `next`, merged as the
    // present side merges it: an image that holds a later frame repaints a
    // part of it. `None` when it is not a part of the window.
    pub(crate) fn repaint(&mut self, next: u64, packet: PacketDamage) -> Option<DamageRects> {
        self.damages.note(next, packet);
        self.damages.union_back(next, self.held?).ok()
    }

    // Notes the window packet `frame` built with `damage`, drawing `draws`.
    pub(crate) fn built(&mut self, frame: u64, damage: PacketDamage, draws: PacketDraws) {
        self.carried = None;
        self.damages.note(frame, damage);
        self.last = Some((frame, draws));
    }

    // The damage of a capture that draws `draws`: none when the window
    // packet built last draws the same.
    pub(crate) fn capture_damage(&self, draws: PacketDraws) -> PacketDamage {
        match self.last {
            Some((frame, last)) if last == draws => PacketDamage {
                since: Some(frame),
                damage: Damage::Part(DamageRects::default()),
            },
            _ => PacketDamage::WHOLE,
        }
    }

    // Notes that the packet `frame`, which changed `damage`, went back
    // unshown.
    pub(crate) fn went_back(&mut self, frame: u64, damage: PacketDamage) {
        if self.last() == Some(frame) {
            self.carried = Some(damage);
        }
    }

    // Notes the oldest window packet the window's targets hold; `None` when
    // one holds none known.
    pub(crate) fn hold(&mut self, held: Option<u64>) {
        self.held = held;
    }
}
