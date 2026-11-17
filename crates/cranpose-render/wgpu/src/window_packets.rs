// The window packets a producer built last: what each changed, the one the
// next changes against, and the oldest one the window's targets hold, which
// a window walk culls against with the parts they draw again.

use cranpose_render_common::scene_store::SceneVersion;

use crate::{
    frame::{Damage, DamageRects, FrameDamages, PacketDamage},
    frame_packet::HeldWindow,
};

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
    held: Option<HeldWindow>,
}

impl WindowPackets {
    pub(crate) fn last(&self) -> Option<u64> {
        self.last.map(|(frame, _)| frame)
    }

    // Writes into `region` the damages since the oldest packet the window's
    // targets hold, the parts they draw again at `landings` and the rects of
    // `begun`: a region a window walk may cull to. `false` when one of the
    // damages is not a part of the window.
    pub(crate) fn cull_region(
        &self,
        begun: [&DamageRects; 2],
        landings: Option<u64>,
        region: &mut DamageRects,
    ) -> bool {
        let Some(last) = self.last() else {
            return false;
        };
        if !self.held_since(last, landings, region) {
            return false;
        }
        for rects in begun {
            region.add_all(rects);
        }
        true
    }

    // Writes into `region` the damages from `at` back to the oldest packet
    // the window's targets hold and, once the count of landings moved past
    // `landings`, the parts they drew with stand-ins, which a frame draws
    // again then; `false` when one of those is not a part of the window. The
    // renderer refuses a culled packet when a landing it counted first makes
    // it draw more.
    fn held_since(&self, at: u64, landings: Option<u64>, region: &mut DamageRects) -> bool {
        let Some(held) = &self.held else {
            return false;
        };
        if self.damages.union_back(at, held.oldest, region).is_err() {
            return false;
        }
        if held.landings.is_none() || held.landings != landings {
            let Damage::Part(stand_ins) = &held.stand_ins else {
                return false;
            };
            region.add_all(stand_ins);
        }
        true
    }

    // Makes `packet`, the damage of the next window packet against the one
    // before it in a window of `viewport`, change against what an unshown
    // packet changed against, when one went back.
    pub(crate) fn carry(&self, packet: &mut PacketDamage, viewport: (u32, u32)) {
        let Some(carried) = &self.carried else {
            return;
        };
        // The carried damage takes the new one in, in the order the merge
        // of rects always took them.
        let mut damage = carried.damage;
        damage.add(&packet.damage, viewport);
        packet.since = carried.since;
        packet.damage = damage;
    }

    // Writes into `region` what an image that holds the oldest frame the
    // window's targets hold repaints for `packet`, the damage of the next
    // window packet `next` in a window of `viewport` before any carried one
    // joins it, at `landings`, merged as the present side merges it: an
    // image that holds a later frame repaints a part of it. `false` when it
    // is not a part of the window.
    pub(crate) fn repaint(
        &mut self,
        next: u64,
        packet: &PacketDamage,
        viewport: (u32, u32),
        landings: Option<u64>,
        region: &mut DamageRects,
    ) -> bool {
        if self.carried.is_some() {
            let mut carried = *packet;
            self.carry(&mut carried, viewport);
            self.damages.note(next, &carried);
        } else {
            self.damages.note(next, packet);
        }
        self.held_since(next, landings, region)
    }

    // Notes the window packet `frame` built with `damage`, drawing `draws`.
    pub(crate) fn built(&mut self, frame: u64, damage: &PacketDamage, draws: PacketDraws) {
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

    // Notes the oldest window packet the window's targets hold, and the
    // parts they draw again; `None` when one holds none known.
    pub(crate) fn hold(&mut self, held: Option<&HeldWindow>) {
        match (held, &mut self.held) {
            (Some(held), Some(kept)) => kept.assign(held),
            (held, kept) => *kept = held.copied(),
        }
    }
}
