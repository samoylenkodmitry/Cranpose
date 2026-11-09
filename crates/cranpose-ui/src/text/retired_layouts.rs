use std::{collections::VecDeque, rc::Rc, sync::Arc};

use super::{AnnotatedString, measure::PreparedTextLayout};

// The most layouts kept, and the most a new layout looks at for one
// nothing holds.
const KEPT: usize = 512;
const LOOKS: usize = 8;

// The prepared layouts of texts that changed, kept for the next changed
// texts. A layout whose lines no scene or packet holds any more takes a new
// text in its allocation and in its lines' storage, so a counter that shows
// a new number every frame allocates no layout: it takes a layout it gave
// up a frame or two before, once a scene and a packet in flight let go of
// its lines. A pass keeps at most three times what the last frame retired
// or took, so the layouts go once texts stop changing.
#[derive(Default)]
pub(crate) struct RetiredLayouts {
    layouts: VecDeque<Rc<PreparedTextLayout>>,
    retired: usize,
    taken: usize,
}

impl RetiredLayouts {
    // Keeps `layout` when nothing else holds it: a layout the shared cache
    // keeps stays there. A kept layout lets go of its string, which the
    // next changed text may take.
    pub(crate) fn retire(&mut self, mut layout: Rc<PreparedTextLayout>) {
        let Some(free) = Rc::get_mut(&mut layout) else {
            return;
        };
        free.text = Arc::clone(AnnotatedString::default().render_string());
        if self.layouts.len() == KEPT {
            self.layouts.pop_front();
        }
        self.layouts.push_back(layout);
        self.retired += 1;
    }

    // The oldest of the first kept layouts that nothing holds, its lines
    // with it; those still held go to the back.
    pub(crate) fn take(&mut self) -> Option<Rc<PreparedTextLayout>> {
        for _ in 0..LOOKS.min(self.layouts.len()) {
            let mut layout = self.layouts.pop_front()?;
            if Rc::get_mut(&mut layout).is_some_and(|free| !free.layout.shares_lines()) {
                self.taken += 1;
                return Some(layout);
            }
            self.layouts.push_back(layout);
        }
        None
    }

    pub(crate) fn begin_pass(&mut self) {
        self.layouts
            .truncate(self.retired.max(self.taken).saturating_mul(3));
        (self.retired, self.taken) = (0, 0);
    }
}
