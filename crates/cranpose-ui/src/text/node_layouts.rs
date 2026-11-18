use std::rc::Rc;

use cranpose_core::PackedU64;
use cranpose_ui_layout::BoundRange;
use smallvec::SmallVec;

use super::{PreparedTextLayout, TextAlign};

// The most layouts of its text a node keeps, at different widths.
const LIVE_LAYOUTS: usize = 4;
// The most layouts of texts it showed before a node keeps for its next
// change: a text that changes each frame lays out into the one it drew two
// frames before, as the scene still draws the one before.
const RETIRED_LAYOUTS: usize = 2;

// A layout of the node's text now, laid out between widths.
#[derive(Clone, Debug)]
pub(crate) struct LiveLayout {
    min_width: f32,
    // The max widths at which the text laid out from `min_width` gives
    // `layout` again.
    max_widths: BoundRange,
    text_generation: PackedU64,
    font_scale_fingerprint: u32,
    layout: Rc<PreparedTextLayout>,
}

impl LiveLayout {
    pub(crate) fn new(
        min_width: f32,
        (text_generation, font_scale_fingerprint): (u64, u32),
        layout: Rc<PreparedTextLayout>,
    ) -> Self {
        Self {
            min_width,
            max_widths: layout.max_widths_kept(min_width),
            text_generation: PackedU64::new(text_generation),
            font_scale_fingerprint,
            layout,
        }
    }

    // Whether the layout stands for the text laid out between `min_width`
    // and `max_width` as it is: its lines and size the same.
    fn keeps(&self, (min_width, max_width): (f32, f32), stamp: (u64, u32)) -> bool {
        self.min_width.to_bits() == min_width.to_bits()
            && self.on(stamp)
            && self.max_widths.contains(max_width)
    }

    // Whether the layout was laid out on the fonts and the font scale of
    // `stamp`.
    fn on(&self, stamp: (u64, u32)) -> bool {
        (self.text_generation.get(), self.font_scale_fingerprint) == stamp
    }

    pub(crate) fn layout(&self) -> &Rc<PreparedTextLayout> {
        &self.layout
    }

    pub(crate) fn max_widths(&self) -> BoundRange {
        self.max_widths
    }
}

// A layout a node holds: of its text now, or of a text it showed before,
// which its next change lays out into once nothing else holds it.
#[derive(Clone, Debug)]
enum NodeLayout {
    Live(LiveLayout),
    Retired(Rc<PreparedTextLayout>),
}

impl NodeLayout {
    fn live(&self) -> Option<&LiveLayout> {
        match self {
            Self::Live(live) => Some(live),
            Self::Retired(_) => None,
        }
    }

    fn into_layout(self) -> Rc<PreparedTextLayout> {
        match self {
            Self::Live(live) => live.layout,
            Self::Retired(layout) => layout,
        }
    }
}

// The layouts a text node holds: those of its text, the latest first,
// then those of texts it showed before, in no order. A text that never
// changes holds one layout, in place. A change moves single entries, by
// a swap or to the end, and never shifts the list.
#[derive(Debug, Default)]
pub(crate) struct NodeLayouts {
    layouts: SmallVec<[NodeLayout; 1]>,
}

impl NodeLayouts {
    // The latest layout of the text, when it stands for the text between
    // `widths` on the fonts of `stamp`.
    pub(crate) fn latest_keeping(
        &self,
        widths: (f32, f32),
        stamp: (u64, u32),
    ) -> Option<&Rc<PreparedTextLayout>> {
        match self.layouts.first() {
            Some(NodeLayout::Live(latest)) if latest.keeps(widths, stamp) => Some(&latest.layout),
            _ => None,
        }
    }

    // The latest layout of the text on the fonts of `stamp`.
    pub(crate) fn latest_on(&self, stamp: (u64, u32)) -> Option<&Rc<PreparedTextLayout>> {
        self.live()
            .next()
            .filter(|latest| latest.on(stamp))
            .map(LiveLayout::layout)
    }

    // The layout of the text that stands for it between `widths`.
    pub(crate) fn keeping(&self, widths: (f32, f32), stamp: (u64, u32)) -> Option<&LiveLayout> {
        self.live().find(|live| live.keeps(widths, stamp))
    }

    fn live(&self) -> impl Iterator<Item = &LiveLayout> {
        self.layouts.iter().map_while(NodeLayout::live)
    }

    fn live_count(&self) -> usize {
        self.live().count()
    }

    // Puts first a layout of the text that takes `widths`: the latest one,
    // which nothing else holds, realigned in place where it breaks into
    // the same lines there, as a width that animates asks for each frame,
    // or another one that keeps them. False when none does.
    pub(crate) fn bring_kept(&mut self, widths: (f32, f32), stamp: (u64, u32)) -> bool {
        let Some(NodeLayout::Live(latest)) = self.layouts.first_mut() else {
            return false;
        };
        // Its style, equal to the source's, gives the alignment.
        if latest.on(stamp)
            && let Some(prepared) = Rc::get_mut(&mut latest.layout)
            && prepared.try_realign(widths, prepared.visual_style.paragraph_style.text_align)
        {
            latest.min_width = widths.0;
            latest.max_widths = prepared.max_widths_kept(widths.0);
            return true;
        }
        let Some(index) = self.live().position(|live| live.keeps(widths, stamp)) else {
            return false;
        };
        self.layouts[..=index].rotate_right(1);
        true
    }

    // The latest layout of the text on the fonts of `stamp` at `widths`,
    // when it breaks into the same lines there, as a width that animates
    // asks for: itself, or a copy with its lines placed across the widths
    // as `align` says, in the allocation of the last layout of the text
    // when the node holds as many as it keeps.
    pub(crate) fn aligned_latest(
        &mut self,
        widths: (f32, f32),
        stamp: (u64, u32),
        align: TextAlign,
    ) -> Option<Rc<PreparedTextLayout>> {
        if !self.latest_on(stamp)?.layout.holds(widths.0, widths.1) {
            return None;
        }
        let count = self.live_count();
        // The last one goes, never the latest.
        let spare = (count >= LIVE_LAYOUTS).then(|| self.layouts.remove(count - 1).into_layout());
        Some(self.latest_on(stamp)?.at_widths(widths, align, spare))
    }

    // Holds `live`, laid out now, as the latest layout of the text; the
    // oldest one goes when the node holds more than it keeps.
    pub(crate) fn insert_latest(&mut self, live: LiveLayout) {
        if let Some(NodeLayout::Live(_)) = self.layouts.first() {
            // Another width of the same text.
            self.layouts.insert(0, NodeLayout::Live(live));
            if self.live_count() > LIVE_LAYOUTS {
                self.layouts.remove(LIVE_LAYOUTS);
            }
            return;
        }
        // After a change the node holds retired layouts only: the new one
        // swaps places with the first of them.
        self.layouts.push(NodeLayout::Live(live));
        let last = self.layouts.len() - 1;
        self.layouts.swap(0, last);
    }

    // The text changed: the latest layout of the old one, which nothing
    // but the scene holds, becomes a layout of a text shown before.
    // Returns what the node gives up: the old text's layouts at other
    // widths, the one a shared cache holds, and the retired ones past those
    // it keeps.
    pub(crate) fn retire_text(&mut self) -> SmallVec<[Rc<PreparedTextLayout>; LIVE_LAYOUTS]> {
        let mut given_up = SmallVec::new();
        let live = self.live_count();
        // From the back, so the entry that comes in from the end lands past
        // the layouts of the text.
        for index in (0..live).rev() {
            let mut layout = self.layouts.swap_remove(index).into_layout();
            match Rc::get_mut(&mut layout) {
                Some(free) if index == 0 => {
                    free.let_go_of_text();
                    self.layouts.push(NodeLayout::Retired(layout));
                }
                _ => given_up.push(layout),
            }
        }
        while self.layouts.len() > RETIRED_LAYOUTS {
            given_up.push(self.layouts.swap_remove(0).into_layout());
        }
        given_up
    }

    // A layout of a text shown before that nothing else holds, which a new
    // text lays out into.
    pub(crate) fn take_refillable(&mut self) -> Option<Rc<PreparedTextLayout>> {
        let live = self.live_count();
        let index = (live..self.layouts.len()).find(|&index| match &mut self.layouts[index] {
            NodeLayout::Retired(layout) => PreparedTextLayout::refillable(layout),
            NodeLayout::Live(_) => false,
        })?;
        Some(self.layouts.swap_remove(index).into_layout())
    }

    // Holds `layout`, a retired one a new text did not lay out into, again.
    pub(crate) fn put_back(&mut self, layout: Rc<PreparedTextLayout>) {
        self.layouts.push(NodeLayout::Retired(layout));
    }

    // The latest layout. Panics when the node holds none: a reader asks
    // after `prepare` or a hit.
    pub(crate) fn latest(&self) -> &Rc<PreparedTextLayout> {
        match &self.layouts[0] {
            NodeLayout::Live(live) => &live.layout,
            NodeLayout::Retired(layout) => layout,
        }
    }
}
