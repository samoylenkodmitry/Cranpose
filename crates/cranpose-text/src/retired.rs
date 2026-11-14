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
/// nothing. The pool keeps at most its bound, the oldest going first, and
/// empties at rest.
#[derive(Debug)]
pub struct Retired<T> {
    items: VecDeque<T>,
    bound: usize,
}

impl<T> Retired<T> {
    /// An empty pool that keeps at most `bound` items.
    pub const fn new(bound: usize) -> Self {
        Self {
            items: VecDeque::new(),
            bound,
        }
    }

    /// Keeps `item` for a later take.
    pub fn retire(&mut self, item: T) {
        if self.items.len() == self.bound {
            self.items.pop_front();
        }
        self.items.push_back(item);
    }

    /// The oldest of the first kept items that `free` takes; those it
    /// refuses go to the back.
    pub fn take(&mut self, mut free: impl FnMut(&mut T) -> bool) -> Option<T> {
        for _ in 0..LOOKS.min(self.items.len()) {
            let mut item = self.items.pop_front()?;
            if free(&mut item) {
                return Some(item);
            }
            self.items.push_back(item);
        }
        None
    }

    /// Drops the kept items and the room they took: the texts rest.
    pub fn clear(&mut self) {
        self.items = VecDeque::new();
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
        if text.is_empty() {
            return AnnotatedString::default();
        }
        if let Some(mut content) = self.take(|content| Arc::get_mut(content).is_some())
            && let Some(render) = Arc::get_mut(&mut content)
        {
            render.refill_plain(text);
            return AnnotatedString::from(content);
        }
        AnnotatedString::from(text)
    }
}
