/// Text baselines measured down from a layout's top edge.
///
/// Containers inherit the smallest first baseline and largest last baseline of
/// their placed children. A missing line remains unspecified through translation.
#[derive(Clone, Copy, Debug)]
pub struct AlignmentLines {
    first: f32,
    last: f32,
}

impl AlignmentLines {
    /// Creates alignment lines. Non-finite coordinates are treated as unspecified.
    pub fn new(first: Option<f32>, last: Option<f32>) -> Self {
        Self {
            first: first.filter(|value| value.is_finite()).unwrap_or(f32::NAN),
            last: last.filter(|value| value.is_finite()).unwrap_or(f32::NAN),
        }
    }

    /// The first drawn text baseline, when this layout provides one.
    pub fn first_baseline(self) -> Option<f32> {
        self.first.is_finite().then_some(self.first)
    }

    /// The last drawn text baseline, when this layout provides one.
    pub fn last_baseline(self) -> Option<f32> {
        self.last.is_finite().then_some(self.last)
    }

    /// Translates specified lines by a child's vertical placement.
    pub fn translated(self, y: f32) -> Self {
        Self {
            first: self.first + y,
            last: self.last + y,
        }
    }

    /// Inherits the first and last lines of another placed child.
    pub fn merge(&mut self, other: Self) {
        self.first = self.first.min(other.first);
        self.last = self.last.max(other.last);
    }

    /// Replaces inherited lines with the lines explicitly provided by a layout.
    pub fn with_overrides(self, explicit: Self) -> Self {
        Self::new(
            explicit.first_baseline().or_else(|| self.first_baseline()),
            explicit.last_baseline().or_else(|| self.last_baseline()),
        )
    }
}

impl Default for AlignmentLines {
    fn default() -> Self {
        Self {
            first: f32::NAN,
            last: f32::NAN,
        }
    }
}

impl PartialEq for AlignmentLines {
    fn eq(&self, other: &Self) -> bool {
        self.first_baseline() == other.first_baseline()
            && self.last_baseline() == other.last_baseline()
    }
}
