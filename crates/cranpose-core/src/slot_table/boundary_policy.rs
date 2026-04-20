use crate::Key;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PassBoundary {
    Open,
    Restored { boundary_key: Key },
    Fresh { boundary_key: Key },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BoundaryPolicy {
    restricted_boundary: Option<Key>,
    exact_live_reuse: bool,
    live_search: bool,
    live_value_reuse: bool,
}

impl BoundaryPolicy {
    pub(crate) fn restricted_boundary(self) -> Option<Key> {
        self.restricted_boundary
    }

    pub(crate) fn inherited_boundary(self, key: Key) -> Key {
        self.restricted_boundary.unwrap_or(key)
    }

    pub(crate) fn allows_exact_live_reuse(self) -> bool {
        self.exact_live_reuse
    }

    pub(crate) fn allows_live_search(self) -> bool {
        self.live_search
    }

    pub(crate) fn disallows_live_value_reuse(self) -> bool {
        !self.live_value_reuse
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BoundaryTransition {
    ReuseLive { boundary_key: Key },
    Restore { boundary_key: Key },
    InsertFresh { boundary_key: Key },
}

impl PassBoundary {
    pub(crate) fn policy(self) -> BoundaryPolicy {
        match self {
            Self::Open => BoundaryPolicy {
                restricted_boundary: None,
                exact_live_reuse: true,
                live_search: true,
                live_value_reuse: true,
            },
            Self::Restored { boundary_key } => BoundaryPolicy {
                restricted_boundary: Some(boundary_key),
                exact_live_reuse: true,
                live_search: false,
                live_value_reuse: true,
            },
            Self::Fresh { boundary_key } => BoundaryPolicy {
                restricted_boundary: Some(boundary_key),
                exact_live_reuse: false,
                live_search: false,
                live_value_reuse: false,
            },
        }
    }

    pub(crate) fn transition(self, transition: BoundaryTransition) -> Self {
        match transition {
            BoundaryTransition::ReuseLive { boundary_key } => match self {
                Self::Open => Self::Open,
                Self::Restored { .. } => Self::Restored { boundary_key },
                Self::Fresh { .. } => Self::Restored { boundary_key },
            },
            BoundaryTransition::Restore { boundary_key } => match self {
                Self::Open | Self::Restored { .. } | Self::Fresh { .. } => {
                    Self::Restored { boundary_key }
                }
            },
            BoundaryTransition::InsertFresh { boundary_key } => Self::Fresh { boundary_key },
        }
    }
}
