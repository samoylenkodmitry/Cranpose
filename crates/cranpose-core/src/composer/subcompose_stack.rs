// The frames of the subcompositions a composer runs, the innermost last.

use crate::{NodeId, RecomposeScope};

#[derive(Default)]
pub(crate) struct SubcomposeFrame {
    pub(crate) nodes: Vec<NodeId>,
    pub(crate) scopes: Vec<RecomposeScope>,
}

// A composer core runs one subcomposition at a time in nearly every pass, so
// the first frame sits in the core and only the frames nested in it take a
// list.
#[derive(Default)]
pub(crate) struct SubcomposeStack {
    first: Option<SubcomposeFrame>,
    nested: Vec<SubcomposeFrame>,
}

impl SubcomposeStack {
    pub(crate) fn push(&mut self, frame: SubcomposeFrame) {
        if self.first.is_none() {
            self.first = Some(frame);
        } else {
            self.nested.push(frame);
        }
    }

    pub(crate) fn pop(&mut self) -> Option<SubcomposeFrame> {
        self.nested.pop().or_else(|| self.first.take())
    }

    pub(crate) fn last(&self) -> Option<&SubcomposeFrame> {
        self.nested.last().or(self.first.as_ref())
    }

    pub(crate) fn last_mut(&mut self) -> Option<&mut SubcomposeFrame> {
        self.nested.last_mut().or(self.first.as_mut())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    pub(crate) fn clear(&mut self) {
        self.first = None;
        self.nested.clear();
    }
}
