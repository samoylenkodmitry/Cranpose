//! Where a panel stands in relation to the panel of the focused text field.

// A panel stands where its modifier and its subspace put it, or below the panel that holds the
// focused text field, as the keyboard that a headset draws does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Dock {
    #[default]
    Free,
    BelowField,
}
