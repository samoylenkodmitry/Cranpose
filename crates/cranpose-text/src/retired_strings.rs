use std::{collections::VecDeque, sync::Arc};

use crate::annotated_string::{AnnotatedString, RenderString};

// The most strings kept, whatever passes asked for, and the most bytes a
// kept string holds: a ticker's text is short, and a kept string holds its
// room.
const KEPT: usize = 4096;
const LONGEST: usize = 128;
// The most kept strings a new string looks at for one nothing holds.
const LOOKS: usize = 8;

// forwards on purpose: the strings go only once texts stop changing, and
// inlined their drops take room in the code each pass runs.
#[cold]
#[inline(never)]
fn drop_past(strings: &mut VecDeque<Arc<RenderString>>, kept: usize) {
    strings.truncate(kept);
}

/// Plain strings that texts gave up while something else, as a scene or a
/// layout in flight, still held them. A plain string made through it takes
/// the oldest one nothing holds any more and writes its text into that
/// room, so a text that shows another string every frame, as a ticker's
/// does, allocates no string. A pass keeps at most three times what the
/// pass before retired or took, so the strings go once texts stop changing.
#[derive(Debug, Default)]
pub struct RetiredStrings {
    strings: VecDeque<Arc<RenderString>>,
    retired: usize,
    taken: usize,
}

impl RetiredStrings {
    /// No strings.
    pub const fn new() -> Self {
        Self {
            strings: VecDeque::new(),
            retired: 0,
            taken: 0,
        }
    }

    /// Whether no string waits for a text to take it.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// A pass of the texts that take and retire strings begins.
    pub fn begin_pass(&mut self) {
        let kept = self.retired.max(self.taken).saturating_mul(3);
        if self.strings.len() > kept {
            drop_past(&mut self.strings, kept);
        }
        (self.retired, self.taken) = (0, 0);
    }

    /// Keeps the room of `text`, a string a text stops showing, for a plain
    /// string made later: only a short string with no styles, annotations
    /// or links.
    pub fn retire(&mut self, text: AnnotatedString) {
        let Some(content) = text.into_plain_content() else {
            return;
        };
        if content
            .plain_capacity()
            .is_none_or(|capacity| capacity > LONGEST)
        {
            return;
        }
        if self.strings.len() == KEPT {
            self.strings.pop_front();
        }
        self.strings.push_back(content);
        self.retired += 1;
    }

    /// A plain string of `text`, in the room of a retired string when
    /// nothing holds one any more.
    pub fn plain(&mut self, text: &str) -> AnnotatedString {
        if text.is_empty() {
            return AnnotatedString::default();
        }
        if let Some(mut content) = self.take_unheld()
            && let Some(render) = Arc::get_mut(&mut content)
        {
            render.refill_plain(text);
            return AnnotatedString::from(content);
        }
        AnnotatedString::from(text)
    }

    // The oldest of the first kept strings that nothing else holds; those
    // still held go to the back.
    fn take_unheld(&mut self) -> Option<Arc<RenderString>> {
        for _ in 0..LOOKS.min(self.strings.len()) {
            let mut content = self.strings.pop_front()?;
            if Arc::get_mut(&mut content).is_some() {
                self.taken += 1;
                return Some(content);
            }
            self.strings.push_back(content);
        }
        None
    }
}
