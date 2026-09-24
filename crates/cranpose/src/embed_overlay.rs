use std::{any::Any, cell::Cell, rc::Rc};

use cranpose_core::remember;
use cranpose_ui::{Box, BoxSpec, Modifier, Size, WindowRootDescriptor, composable};

pub(crate) struct HostOverlayRoot {
    anchor: String,
    size: Cell<Size>,
}

impl HostOverlayRoot {
    pub(crate) fn new(anchor: impl Into<String>) -> Self {
        Self {
            anchor: anchor.into(),
            size: Cell::new(Size::new(0.0, 0.0)),
        }
    }

    pub(crate) fn anchor(&self) -> &str {
        &self.anchor
    }

    pub(crate) fn set_size(&self, size: Size) {
        self.size.set(size);
    }
}

impl WindowRootDescriptor for HostOverlayRoot {
    fn layout_size(&self) -> Size {
        self.size.get()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Draws `content` on a transparent layer the host lays over one of its own
/// views, named by `anchor`, and sizes to that view.
///
/// The layer takes no input: pointer events reach the view underneath, so
/// it suits effects drawn on top of something the host shows, such as
/// particles over an editor's text. What anchors exist is for the host to
/// say; the IntelliJ plugin template offers `"editor"`, the focused editor.
/// Outside a host the content is composed but drawn nowhere.
#[composable]
pub fn HostOverlay(anchor: &'static str, content: impl FnMut() + 'static) {
    let root = remember(|| Rc::new(HostOverlayRoot::new(anchor))).with(Rc::clone);
    let descriptor: Rc<dyn WindowRootDescriptor> = root;
    Box(
        Modifier::empty().window_root(descriptor),
        BoxSpec::default(),
        content,
    );
}

#[cfg(test)]
#[path = "tests/embed_overlay_tests.rs"]
mod tests;
