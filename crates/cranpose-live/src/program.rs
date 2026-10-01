use std::{collections::BTreeMap, rc::Rc};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An expression evaluated against registered view-model state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expression {
    /// A scalar constant.
    Literal {
        /// Constant value.
        value: Value,
    },
    /// The current value of a registered state flow.
    State {
        /// Bound view-model name.
        api: String,
        /// Exported state name.
        member: String,
    },
    /// Convert a scalar expression to display text.
    Text {
        /// Expression to format.
        value: Box<Expression>,
    },
    /// An event calling a registered action.
    Action {
        /// Bound view-model name.
        api: String,
        /// Exported action name.
        member: String,
        /// Arguments evaluated when the event is invoked.
        arguments: Vec<Expression>,
    },
}

/// One component invocation with a stable document identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Identity retained across source and structured edits.
    pub id: String,
    /// Qualified component name, or an unambiguous short name.
    pub component: String,
    /// Named arguments matching the component schema.
    #[serde(default)]
    pub arguments: BTreeMap<String, Expression>,
    /// The component's content slot.
    #[serde(default)]
    pub children: Vec<Rc<Node>>,
}

/// An editable UI document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    /// Root component.
    pub root: Rc<Node>,
}

/// An atomic document edit.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    /// Replace a subtree, preserving sibling identities.
    Replace {
        /// Existing node to replace.
        target: String,
        /// Replacement subtree.
        node: Rc<Node>,
    },
    /// Replace one component argument.
    SetArgument {
        /// Existing node to edit.
        target: String,
        /// Parameter name.
        name: String,
        /// Replacement expression.
        value: Expression,
    },
}

/// A version-checked transaction submitted by an editor or agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// Document revision the edits were based on.
    pub base_revision: u64,
    /// Edits applied together, or rejected together.
    pub edits: Vec<Edit>,
}

impl Program {
    pub(crate) fn edited(&self, edits: Vec<Edit>) -> Result<Self, crate::Error> {
        let mut next = self.clone();
        for edit in edits {
            let target = match &edit {
                Edit::Replace { target, .. } | Edit::SetArgument { target, .. } => target,
            };
            if !contains(&next.root, target) {
                return Err(crate::Error::Invalid(format!("unknown node '{target}'")));
            }
            apply(&mut next.root, edit);
        }
        Ok(next)
    }
}

fn contains(node: &Node, target: &str) -> bool {
    node.id == target || node.children.iter().any(|child| contains(child, target))
}

fn apply(node: &mut Rc<Node>, edit: Edit) {
    let target = match &edit {
        Edit::Replace { target, .. } | Edit::SetArgument { target, .. } => target,
    };
    if node.id == *target {
        match edit {
            Edit::Replace {
                node: replacement, ..
            } => *node = replacement,
            Edit::SetArgument { name, value, .. } => {
                Rc::make_mut(node).arguments.insert(name, value);
            }
        }
    } else if let Some(index) = node
        .children
        .iter()
        .position(|child| contains(child, target))
    {
        apply(&mut Rc::make_mut(node).children[index], edit);
    }
}
