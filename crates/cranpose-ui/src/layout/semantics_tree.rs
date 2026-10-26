use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_foundation::{SemanticsCustomAction, SemanticsWidgetRole};

use super::{GeometryRect, LayoutState, Point, SemanticsDetails, Size};

/// Discrete event callback reference produced during semantics extraction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticsCallback {
    pub(super) node_id: NodeId,
}

impl SemanticsCallback {
    /// Creates a callback that targets the node with `node_id`.
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    /// Returns the id of the node whose action this callback triggers.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }
}

/// Semantics action exposed to the input system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticsAction {
    /// Activates the node, as a tap or a keyboard or assistive technology click does.
    Click {
        /// Callback that identifies the node to activate.
        handler: SemanticsCallback,
    },
}

/// A text node's text in the semantics tree, shared with the node's modifier
/// slices rather than copied out of them.
#[derive(Clone, Debug)]
pub struct SemanticsText(pub(super) Rc<crate::text::AnnotatedString>);

impl SemanticsText {
    /// The text the node shows.
    pub fn as_str(&self) -> &str {
        &self.0.text
    }
}

impl PartialEq for SemanticsText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for SemanticsText {}

impl From<&str> for SemanticsText {
    fn from(text: &str) -> Self {
        Self(Rc::new(crate::text::AnnotatedString::from(text)))
    }
}

/// Semantic role describing how a node should participate in accessibility and hit testing.
/// Roles are now derived from SemanticsConfiguration rather than widget types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticsRole {
    /// Generic container or layout node
    Layout,
    /// Subcomposition boundary
    Subcompose,
    /// Text content derived from the text node semantics payload.
    Text {
        /// The text the node shows.
        value: SemanticsText,
    },
    /// Spacer (non-interactive)
    Spacer,
    /// Button (derived from the semantics `role`)
    Button,
    /// Unknown or unspecified role
    Unknown,
}

/// A single node within the semantics tree.
///
/// Not `Eq`: a node now carries drawn-control bounds, which are floats. The
/// tree is compared for "did anything a screen reader can see change", and
/// that comparison is `PartialEq` everywhere else on the accessibility path
/// (`AccessibilityRect`, `AccessibilityElement`) for the same reason.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsNode {
    /// Id of the runtime node this semantics node describes.
    pub node_id: NodeId,
    /// Where the node lies in the root's coordinates: its layout rect,
    /// before graphics-layer transforms, as [`LayoutBox::rect`] holds it.
    pub bounds: GeometryRect,
    /// Where layout put the node inside its parent's content, from which a
    /// tree update moves a node that did not change along with its parent.
    pub placement: SemanticsPlacement,
    /// Incarnation of the runtime node, incremented when its storage is recycled.
    pub node_generation: u32,
    /// Where this node sits in the tree (layout, text, subcomposition …).
    pub role: SemanticsRole,
    /// What kind of control this node is, as a screen reader announces it —
    /// Compose's `Role`. Separate from [`SemanticsNode::role`] because a
    /// clickable `Row` is structurally a layout node and semantically a button.
    pub widget_role: Option<SemanticsWidgetRole>,
    /// Actions that the input system and assistive technology can invoke on this node.
    pub actions: Vec<SemanticsAction>,
    /// Child semantics nodes, in layout order.
    pub children: Vec<Self>,
    /// Content description that a screen reader announces for the node, if one was set.
    pub description: Option<String>,
    /// Direct activation callback for keyboard and assistive technology.
    pub on_click: Option<SemanticsCustomAction>,
    /// Selection state, or `None` if the node is not selectable.
    pub selected: Option<bool>,
    /// Checked state, or `None` if the node is not a toggleable control.
    pub toggled: Option<cranpose_foundation::ToggleableState>,
    /// Whether the node accepts user interaction. `true` unless the node was disabled.
    pub enabled: bool,
    /// Whether a screen reader skips this node and everything under it.
    pub hidden: bool,
    /// Whether a screen reader takes this node and the text under it as one
    /// stop.
    pub merge_descendants: bool,
    /// Where a screen reader visits this node among the ones beside it.
    pub traversal_index: f32,
    /// The text an editable field holds.
    pub text: Option<String>,
    /// Whether this node registered a focus target, so focus can land on it.
    /// Compose's `SemanticsProperties.Focused` companion.
    pub focusable: bool,
    /// Whether focus sits on this node right now.
    pub focused: bool,
    /// The properties few nodes set, held only when the node sets one: read
    /// them through [`SemanticsNode::details`] and write them through
    /// [`SemanticsNode::set_details`].
    pub details: Option<Box<SemanticsDetails>>,
}

impl Default for SemanticsNode {
    fn default() -> Self {
        Self {
            node_id: NodeId::default(),
            bounds: GeometryRect::EMPTY,
            placement: SemanticsPlacement::default(),
            node_generation: 0,
            role: SemanticsRole::Unknown,
            widget_role: None,
            actions: Vec::new(),
            children: Vec::new(),
            description: None,
            on_click: None,
            selected: None,
            toggled: None,
            enabled: true,
            hidden: false,
            merge_descendants: false,
            traversal_index: 0.0,
            text: None,
            focusable: false,
            focused: false,
            details: None,
        }
    }
}

/// Where layout put a semantics node: its offset inside its parent's
/// content and where its own content starts inside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SemanticsPlacement {
    pub(super) position: Point,
    pub(super) content_offset: Point,
}

impl SemanticsPlacement {
    pub(super) fn of(state: &LayoutState) -> Self {
        Self {
            position: state.position(),
            content_offset: state.content_offset(),
        }
    }

    pub(super) fn place(self, origin: Point) -> (Point, Point) {
        let top_left = Point {
            x: origin.x + self.position.x,
            y: origin.y + self.position.y,
        };
        let content = Point {
            x: top_left.x + self.content_offset.x,
            y: top_left.y + self.content_offset.y,
        };
        (top_left, content)
    }
}

/// Rooted semantics tree extracted after layout.
///
/// Two trees are equal when they hold the same nodes; what a tree keeps to
/// bring itself up to date is not compared.
#[derive(Clone, Debug)]
pub struct SemanticsTree {
    pub(super) full: SemanticsNode,
    pub(super) modal: Option<Vec<usize>>,
    pub(super) tracking: super::semantics_update::SemanticsTracking,
}

impl PartialEq for SemanticsTree {
    fn eq(&self, other: &Self) -> bool {
        self.full == other.full && self.modal == other.modal
    }
}

impl SemanticsTree {
    pub(super) fn new(full: SemanticsNode) -> Self {
        let modal = top_modal_path(&full);
        Self {
            full,
            modal,
            tracking: super::semantics_update::SemanticsTracking::default(),
        }
    }

    /// Returns the top visible modal subtree, or the full root when no modal is open.
    pub fn root(&self) -> &SemanticsNode {
        self.modal.as_deref().map_or(&self.full, |path| {
            path.iter()
                .try_fold(&self.full, |node, &index| node.children.get(index))
                .unwrap_or(&self.full)
        })
    }
}

/// Whether a modal of `size` takes space; a zero-sized modal hides nothing.
pub(super) fn modal_takes_space(size: Size) -> bool {
    size.width > 0.0 && size.height > 0.0 && size.width.is_finite() && size.height.is_finite()
}

/// The path to the modal drawn on top: the last visible modal in document
/// order, a modal inside another preferred to it.
pub(super) fn top_modal_path(root: &SemanticsNode) -> Option<Vec<usize>> {
    pub(super) fn search(node: &SemanticsNode, path: &mut Vec<usize>) -> bool {
        if node.hidden {
            return false;
        }
        for (index, child) in node.children.iter().enumerate().rev() {
            path.push(index);
            if search(child, path) {
                return true;
            }
            path.pop();
        }
        node.details().is_modal
    }
    let mut path = Vec::new();
    search(root, &mut path).then_some(path)
}
