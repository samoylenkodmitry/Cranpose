use cranpose_ui_graphics::{CursorIcon, PointerIcon};

use super::{Modifier, inspector_metadata};
use crate::modifier_nodes::PointerIconElement;

impl Modifier {
    /// Names the pointer's appearance while it hovers this node.
    ///
    /// The innermost region under the pointer wins, so a control inside a
    /// window can ask for its own icon without the window's icon overriding it.
    /// Declaring an icon makes the node a hit target in its own right, which is
    /// what lets a decorative region — a title bar, a display panel — carry a
    /// cursor without also handling clicks.
    ///
    /// Platforms without a pointing device (Android, iOS) ignore it.
    ///
    /// Example: `Modifier::empty().pointer_icon(PointerIcon::POINTER)`
    pub fn pointer_icon(self, icon: PointerIcon) -> Self {
        let name = icon
            .css_keyword()
            .map(str::to_string)
            .unwrap_or_else(|| "custom".to_string());
        let modifier = Self::with_element(PointerIconElement::new(icon)).with_inspector_metadata(
            inspector_metadata("pointerIcon", move |info| {
                info.add_property("pointerIcon", &name);
            }),
        );
        self.then(modifier)
    }

    /// Names one of the platform's standard pointer shapes while it hovers this
    /// node, the common case of [`pointer_icon`](Self::pointer_icon).
    ///
    /// Example: `Modifier::empty().cursor(CursorIcon::EwResize)`
    pub fn cursor(self, icon: CursorIcon) -> Self {
        self.pointer_icon(PointerIcon::System(icon))
    }
}
