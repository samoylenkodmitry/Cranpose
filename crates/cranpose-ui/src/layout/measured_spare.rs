// The storage of a node's last measurement, for its next measure to fill.

use std::rc::Rc;

use super::{MeasuredChild, MeasuredNode};
use crate::widgets::nodes::NodeCache;

/// The memory of a node's last measurement, children's list included, for
/// its next measure to fill. It is taken only when nothing else holds the
/// measurement.
#[derive(Default)]
pub(super) struct MeasuredSpare {
    pub(super) node: Option<Rc<MeasuredNode>>,
    pub(super) children: Vec<MeasuredChild>,
}

impl MeasuredSpare {
    pub(super) fn take(cache: &NodeCache) -> Self {
        let Some(mut node) = cache.take_previous_measurement() else {
            return Self::default();
        };
        let Some(previous) = Rc::get_mut(&mut node) else {
            return Self::default();
        };
        let mut children = std::mem::take(&mut previous.children);
        children.clear();
        Self {
            node: Some(node),
            children,
        }
    }
}
