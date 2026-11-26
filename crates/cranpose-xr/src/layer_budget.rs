//! Which layers of a frame keep a layer of the compositor of their own when the frame has more
//! layers than the runtime takes.
//!
//! The room's layer, the streamed scenes, the projection layer of the 3D content and the rays keep
//! their layers in every frame. A surface keeps its layers too, because only the compositor reads
//! the pictures that its producer writes. The other layers hold pictures that the app draws: the
//! content of the panels and their handles. When a frame has more layers than the runtime takes,
//! the projection layer draws some of those pictures, so no panel goes missing. The handles go into
//! it first, and then the panels of the lowest rank.
//!
//! A panel ranks by its layer priority first: a higher priority ranks higher. Panels of one
//! priority rank by whether a hand points at them, and then by their distance from the head: the
//! nearer panel ranks higher. A panel that the last frame drew into the projection layer counts
//! as `SWAP_MARGIN` farther than it stands, so two panels at about one distance do not swap their
//! places at every small move of the head. A frame with no projection layer gets one, and that
//! layer takes one place of the budget. Surfaces that do not fit beside the fixed layers are left
//! out, the farthest first.

// How much nearer than a panel with a layer of its own a panel from the projection layer must
// stand to take that layer, in meters.
const SWAP_MARGIN: f32 = 0.05;

// What one panel adds to a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PanelNeed {
    pub(crate) id: u64,
    // The distance of the panel's plane from the head, in meters
    pub(crate) distance: f32,
    // The priority that the app gave the panel's layer
    pub(crate) priority: i32,
    // Whether a hand points at the panel or holds one of its handles
    pub(crate) pointed: bool,
    // Whether the panel shows content that the app draws, on a layer of its own or in the
    // projection layer
    pub(crate) content: bool,
    // The layers of the panel's surface: one, or one for each eye. Only the compositor shows them.
    pub(crate) surfaces: usize,
    // The layers of the panel's handles, one for each
    pub(crate) handles: usize,
}

impl PanelNeed {
    fn layers(&self) -> usize {
        usize::from(self.content) + self.surfaces + self.handles
    }
}

// Where a frame shows a part of a panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Place {
    // On a layer of its own
    #[default]
    Own,
    // In the projection layer
    Projected,
    // Nowhere: no layer is left, and the projection layer cannot draw it
    LeftOut,
}

// Where a frame shows the parts of one panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) content: Place,
    pub(crate) surfaces: Place,
    pub(crate) handles: Place,
}

// What the projection layer of the 3D content can do in a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Projection {
    // The frame has no 3D content, and it gets the layer when it draws other pictures into it
    #[default]
    Absent,
    // The frame has the layer for its 3D content
    Present,
    // The frame cannot have the layer, because the 3D content failed
    Unavailable,
}

// The layers of a frame that belong to no panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FixedLayers {
    // The room's layer, the streamed scenes and the rays
    pub(crate) layers: usize,
    pub(crate) projection: Projection,
}

// What a plan gives a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Budget {
    // The layers that the frame needs when every picture has a layer of its own
    pub(crate) needed: usize,
    // The layers that the frame gets, the projection layer included when it has one
    pub(crate) layers: usize,
    // The layers whose pictures the projection layer draws instead
    pub(crate) projected: usize,
    // The layers that the frame leaves out
    pub(crate) left_out: usize,
}

// The plan of a frame's layers. The lists keep their storage from frame to frame.
#[derive(Default)]
pub(crate) struct LayerBudget {
    needs: Vec<PanelNeed>,
    placements: Vec<Placement>,
    // The panels with content, from the highest rank to the lowest, with the distance that ranks
    // them
    ranked: Vec<(usize, f32)>,
    // The panels whose content the last plan drew into the projection layer
    projected: Vec<u64>,
    // The plan that the log named last, so that it names each change once
    logged: Budget,
}

impl LayerBudget {
    // Starts the plan of a frame with no panel.
    pub(crate) fn clear(&mut self) {
        self.needs.clear();
    }

    // Adds the next panel in the order in which the compositor draws them: the farthest first.
    pub(crate) fn push(&mut self, need: PanelNeed) {
        self.needs.push(need);
    }

    // Plans the frame for a runtime that takes `limit` layers. The log names each change of the
    // plan.
    pub(crate) fn plan(&mut self, limit: usize, fixed: FixedLayers) -> Budget {
        let budget = self.place(limit, fixed);
        if budget != self.logged {
            self.logged = budget;
            log::info!(
                "cranpose xr: the frame needs {} layers and gets {} of the {limit} that the runtime takes: the projection layer draws {} of them, {} are left out",
                budget.needed,
                budget.layers,
                budget.projected,
                budget.left_out
            );
        }
        budget
    }

    fn place(&mut self, limit: usize, fixed: FixedLayers) -> Budget {
        self.placements.clear();
        self.placements
            .resize(self.needs.len(), Placement::default());
        let projection = usize::from(fixed.projection == Projection::Present);
        let needed =
            fixed.layers + projection + self.needs.iter().map(PanelNeed::layers).sum::<usize>();
        if needed <= limit {
            self.projected.clear();
            return Budget {
                needed,
                layers: needed,
                projected: 0,
                left_out: 0,
            };
        }
        let (fixed_layers, drawn) = match fixed.projection {
            Projection::Absent | Projection::Present => (fixed.layers + 1, Place::Projected),
            Projection::Unavailable => (fixed.layers, Place::LeftOut),
        };
        let mut room = limit.saturating_sub(fixed_layers);
        let mut left_out = 0;
        // The nearest surfaces keep their layers while they fit
        for (need, placement) in self.needs.iter().zip(&mut self.placements).rev() {
            if need.surfaces == 0 {
                continue;
            }
            if need.surfaces <= room {
                room -= need.surfaces;
            } else {
                placement.surfaces = Place::LeftOut;
                left_out += need.surfaces;
            }
        }
        self.ranked.clear();
        let before = &self.projected;
        self.ranked.extend(
            self.needs
                .iter()
                .enumerate()
                .filter(|(_, need)| need.content)
                .map(|(index, need)| {
                    let margin = if before.contains(&need.id) {
                        SWAP_MARGIN
                    } else {
                        0.0
                    };
                    (index, need.distance + margin)
                }),
        );
        let needs = &self.needs;
        self.ranked.sort_unstable_by(|&(a, far_a), &(b, far_b)| {
            let (a_need, b_need) = (&needs[a], &needs[b]);
            b_need
                .priority
                .cmp(&a_need.priority)
                .then(b_need.pointed.cmp(&a_need.pointed))
                .then(far_a.total_cmp(&far_b))
                .then(b.cmp(&a))
        });
        self.projected.clear();
        let mut projected = 0;
        for &(index, _) in &self.ranked {
            if room > 0 {
                room -= 1;
            } else {
                self.placements[index].content = drawn;
                self.projected.push(self.needs[index].id);
                projected += 1;
            }
        }
        // Handles keep their layers only when every panel kept its own, the nearest first
        for (need, placement) in self.needs.iter().zip(&mut self.placements).rev() {
            if need.handles == 0 {
                continue;
            }
            if need.handles <= room {
                room -= need.handles;
            } else {
                placement.handles = drawn;
                projected += need.handles;
            }
        }
        let (projected, left_out) = match drawn {
            Place::LeftOut => (0, left_out + projected),
            Place::Own | Place::Projected => (projected, left_out),
        };
        let has_projection = fixed.projection == Projection::Present || projected > 0;
        Budget {
            needed,
            layers: needed + usize::from(has_projection) - projection - projected - left_out,
            projected,
            left_out,
        }
    }

    // Where the frame shows the parts of the panel at `index` in the order of `push`.
    pub(crate) fn placement(&self, index: usize) -> Placement {
        self.placements.get(index).copied().unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "tests/layer_budget_tests.rs"]
mod tests;
