//! The layout flags of a node and of the nodes below it.

/// What a node below a node needs: layout, and measure when `measure` is set.
#[derive(Clone, Copy, Default)]
pub(crate) struct DescendantDirt {
    pub(crate) layout: bool,
    pub(crate) measure: bool,
}

impl DescendantDirt {
    pub(crate) fn marked(self, measure: bool) -> Self {
        Self {
            layout: true,
            measure: self.measure || measure,
        }
    }
}

/// Whether a layout node, or a node below it, needs measure or layout.
#[derive(Clone, Copy)]
pub(crate) struct LayoutDirt {
    pub(crate) measure: bool,
    pub(crate) layout: bool,
    pub(crate) descendant: DescendantDirt,
}

impl LayoutDirt {
    /// Nothing at or below the node needs measure or layout.
    pub(crate) const CLEAN: Self = Self {
        measure: false,
        layout: false,
        descendant: DescendantDirt {
            layout: false,
            measure: false,
        },
    };

    /// Whether the node or a node below it needs layout.
    pub(crate) fn any(self) -> bool {
        self.measure || self.layout || self.descendant.layout
    }
}

/// A new node needs a measure, and a cache whose node keeps its flags
/// elsewhere reads as dirty.
impl Default for LayoutDirt {
    fn default() -> Self {
        Self {
            measure: true,
            layout: true,
            descendant: DescendantDirt::default(),
        }
    }
}
