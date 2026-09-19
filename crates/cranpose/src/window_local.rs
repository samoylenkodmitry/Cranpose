use std::{cell::RefCell, fmt};

use cranpose_core::{CompositionLocal, ProvidedValue, compositionLocalOf};
use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState,
};
use cranpose_ui::Modifier;

use crate::native_window::WindowState;

thread_local! {
    static WINDOW_STATE: RefCell<Option<CompositionLocal<Option<WindowState>>>> =
        const { RefCell::new(None) };
}

fn window_state_local() -> CompositionLocal<Option<WindowState>> {
    WINDOW_STATE.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| None))
            .clone()
    })
}

/// The window a composable is drawn in.
///
/// [`WindowModifierExt::window`](crate::WindowModifierExt::window) provides
/// this to everything composed inside the window it declares, on every
/// platform, so window content reads the window it lives in rather than
/// taking it as an argument:
///
/// ```rust,ignore
/// Box(
///     Modifier::empty().window(WindowConfig::borderless_for_state("Pet", state)),
///     BoxSpec::default(),
///     || Pet(),
/// );
///
/// #[composable]
/// fn Pet() {
///     let window = LocalWindowState::current();
///     let at = window.and_then(WindowState::position);
/// }
/// ```
pub struct LocalWindowState;

impl LocalWindowState {
    /// The nearest enclosing window's state, or `None` for content that is
    /// not inside a window of its own.
    ///
    /// Reading it subscribes the calling composable to changes the way any
    /// other composition local does.
    pub fn current() -> Option<WindowState> {
        window_state_local().current()
    }

    /// Hands a window state to a subtree. The window modifier does this
    /// itself; call it to stand a subtree in for a window of its own.
    #[track_caller]
    pub fn provides(state: Option<WindowState>) -> ProvidedValue {
        window_state_local().provides(state)
    }
}

pub(crate) fn with_window_state_local(modifier: Modifier, state: Option<WindowState>) -> Modifier {
    modifier.then(Modifier::with_element(WindowStateLocalElement { state }))
}

struct WindowStateLocalElement {
    state: Option<WindowState>,
}

impl fmt::Debug for WindowStateLocalElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowStateLocalElement")
            .field("declared", &self.state.is_some())
            .finish()
    }
}

impl PartialEq for WindowStateLocalElement {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}

impl std::hash::Hash for WindowStateLocalElement {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        self.state.is_some().hash(hasher);
    }
}

impl ModifierNodeElement for WindowStateLocalElement {
    type Node = WindowStateLocalNode;

    fn create(&self) -> Self::Node {
        WindowStateLocalNode {
            state: NodeState::new(),
        }
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::empty()
    }

    fn inspector_name(&self) -> &'static str {
        "windowState"
    }

    fn provided_composition_locals(&self) -> Vec<ProvidedValue> {
        vec![LocalWindowState::provides(self.state)]
    }
}

pub(crate) struct WindowStateLocalNode {
    state: NodeState,
}

impl DelegatableNode for WindowStateLocalNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for WindowStateLocalNode {}
