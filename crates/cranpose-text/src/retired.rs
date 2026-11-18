use std::{collections::VecDeque, sync::Arc};

use crate::annotated_string::{AnnotatedString, RenderString};

// The most kept items a take looks at for one nothing holds.
const LOOKS: usize = 8;
// The most bytes a kept string holds: a ticker's text is short, and a kept
// string holds its room.
const LONGEST: usize = 128;

/// What texts gave up while something else, as a scene or a layout in
/// flight, may still hold it, kept for the texts that change next: a text
/// that shows another string every frame, as a ticker's does, takes what it
/// gave up a frame or two before once nothing holds it, and allocates
/// nothing. The pool keeps about what texts gave up in its recent passes,
/// at most its bound, the oldest going first, and empties at rest.
#[derive(Debug)]
pub struct Retired<T> {
    items: VecDeque<T>,
    // How many items were kept when the current span of recent passes began,
    // less those taken or pushed out since: as many leave from the front
    // when the span ends.
    old: usize,
    // The passes since the span began.
    passes: u64,
    bound: usize,
}

impl<T> Retired<T> {
    /// An empty pool that keeps at most `bound` items.
    pub const fn new(bound: usize) -> Self {
        Self {
            items: VecDeque::new(),
            old: 0,
            passes: 0,
            bound,
        }
    }

    /// Keeps `item` for a later take.
    pub fn retire(&mut self, item: T) {
        if self.items.len() == self.bound {
            self.items.pop_front();
            self.old = self.old.saturating_sub(1);
        }
        self.items.push_back(item);
    }

    /// The oldest of the first kept items that `free` takes; those it
    /// refuses go to the back.
    pub fn take(&mut self, mut free: impl FnMut(&mut T) -> bool) -> Option<T> {
        for _ in 0..LOOKS.min(self.items.len()) {
            let mut item = self.items.pop_front()?;
            if free(&mut item) {
                self.old = self.old.saturating_sub(1);
                return Some(item);
            }
            self.items.push_back(item);
        }
        None
    }

    /// Begins a pass. Each `recent` passes, as many items leave from the
    /// front as were kept `recent` passes before and not taken since, so
    /// the pool keeps as many items as texts gave up in its last `recent`
    /// passes, and in the passes of a span at most twice that.
    pub fn begin_pass(&mut self, recent: u64) {
        self.passes += 1;
        if self.passes >= recent {
            self.passes = 0;
            let old = self.old.min(self.items.len());
            self.items.drain(..old);
            self.old = self.items.len();
        }
    }

    /// Drops the kept items and the room they took: the texts rest.
    pub fn clear(&mut self) {
        *self = Self::new(self.bound);
    }
}

impl Retired<Arc<RenderString>> {
    /// Keeps the room of `text`, a string a text stops showing, for a plain
    /// string made later: only a short string with no styles, annotations
    /// or links.
    pub fn retire_text(&mut self, text: AnnotatedString) {
        let Some(content) = text.into_plain_content() else {
            return;
        };
        if content
            .plain_capacity()
            .is_some_and(|capacity| capacity <= LONGEST)
        {
            self.retire(content);
        }
    }

    /// A plain string of `text`, in the room of a retired string when
    /// nothing holds one any more.
    pub fn plain(&mut self, text: &str) -> AnnotatedString {
        self.plain_of(Plain::Borrowed(text))
    }

    /// [`Self::plain`] of a string the caller gives up, whose room the
    /// string keeps: a retired string nothing holds gives its shared block
    /// and lets its own room go.
    pub fn plain_owned(&mut self, text: String) -> AnnotatedString {
        self.plain_of(Plain::Owned(text))
    }

    // A plain string of `text`, in a retired string nothing holds any more
    // when one is kept. Empty text takes none. One body for both kinds of
    // text, so a build holds one copy of the search.
    fn plain_of(&mut self, text: Plain<'_>) -> AnnotatedString {
        if text.as_str().is_empty() {
            return AnnotatedString::default();
        }
        if let Some(mut content) = self.take(|content| Arc::get_mut(content).is_some())
            && let Some(render) = Arc::get_mut(&mut content)
        {
            match text {
                Plain::Borrowed(text) => render.refill_plain(text),
                Plain::Owned(text) => render.replace_plain(text),
            }
            return AnnotatedString::from(content);
        }
        match text {
            Plain::Borrowed(text) => AnnotatedString::from(text),
            Plain::Owned(text) => AnnotatedString::new(text),
        }
    }
}

// A plain text a retired string takes: copied, or given up with its room.
enum Plain<'a> {
    Borrowed(&'a str),
    Owned(String),
}

impl Plain<'_> {
    fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::Owned(text) => text,
        }
    }
}
