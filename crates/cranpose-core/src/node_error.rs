/// An error from an applier or from applying composition commands to nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    /// No node has this id.
    Missing {
        /// The id that was looked up.
        id: NodeId,
    },
    /// The node's type differs from the type that the caller requested.
    TypeMismatch {
        /// The id of the node.
        id: NodeId,
        /// The name of the type that the caller requested.
        expected: &'static str,
    },
    /// The node lacks the context that an operation needs.
    MissingContext {
        /// The id of the node.
        id: NodeId,
        /// What the node lacks.
        reason: &'static str,
    },
    /// A node with this id exists already.
    AlreadyExists {
        /// The id that is already in use.
        id: NodeId,
    },
    /// A queued command has no payload or the wrong one.
    MalformedCommandPayload {
        /// The label of the command whose payload is wrong.
        tag: &'static str,
    },
    /// A slot host could not be used for `operation`.
    SlotHostUnavailable {
        /// The operation that needed the host.
        operation: &'static str,
        /// Why the host was unavailable.
        reason: &'static str,
    },
    /// Reconciling a composition did not settle within `limit` iterations.
    RecompositionLimitExceeded {
        /// The loop that reached the limit.
        operation: &'static str,
        /// The most iterations allowed.
        limit: usize,
    },
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { id } => write!(f, "node {id} missing"),
            Self::TypeMismatch { id, expected } => {
                write!(f, "node {id} type mismatch; expected {expected}")
            }
            Self::MissingContext { id, reason } => {
                write!(f, "missing context for node {id}: {reason}")
            }
            Self::AlreadyExists { id } => {
                write!(f, "node {id} already exists")
            }
            Self::MalformedCommandPayload { tag } => {
                write!(f, "command queue missing or invalid {tag} payload")
            }
            Self::SlotHostUnavailable { operation, reason } => {
                write!(f, "{operation} cannot access slot host: {reason}")
            }
            Self::RecompositionLimitExceeded { operation, limit } => {
                write!(
                    f,
                    "{operation} exceeded {limit} iterations while reconciling composition"
                )
            }
        }
    }
}

impl std::error::Error for NodeError {}

use crate::NodeId;
