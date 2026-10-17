//! Spacer widget implementation

use cranpose_core::NodeId;

use crate::{
    composable,
    modifier::Modifier,
    widgets::layout::{LayoutEnvironment, compose_empty_layout},
};

/// Reserves layout space with the supplied modifier.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn SpacedLabels() {
///     Row(Modifier::empty(), RowSpec::default(), || {
///         Text("Left", Modifier::empty(), TextStyle::default());
///         Spacer(Modifier::empty().width(16.0));
///         Text("Right", Modifier::empty(), TextStyle::default());
///     });
/// }
/// ```
///
/// Like Compose's, it is not restartable: it composes in its caller's group.
#[composable(no_skip)]
pub fn Spacer(modifier: Modifier) -> NodeId {
    compose_empty_layout(modifier, LayoutEnvironment::current())
}
