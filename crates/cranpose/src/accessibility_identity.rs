use cranpose_core::{NodeId, collections::map::HashMap};

use crate::accessibility::AccessibilityElement;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum AccessibilityIdentityError {
    #[error("accessibility node IDs exhausted")]
    Exhausted,
    #[error("duplicate accessibility identity: node {0}, canvas key {1:?}")]
    Duplicate(NodeId, Option<u64>),
}

/// What [`AccessibilitySnapshot::update`] replaced: the elements and ids
/// published before, and for each new element the index its control had
/// among them.
#[cfg_attr(
    not(any(test, target_os = "android", target_os = "ios")),
    allow(
        dead_code,
        reason = "only the Android and iOS bridges diff against what an update replaced"
    )
)]
pub(crate) struct Replaced {
    pub(crate) elements: Vec<AccessibilityElement>,
    #[cfg_attr(
        not(any(test, target_os = "android")),
        allow(
            dead_code,
            reason = "only the Android wire resends an order that changed"
        )
    )]
    pub(crate) ids: Vec<i32>,
    pub(crate) was: Vec<Option<usize>>,
}

#[derive(Default)]
pub(crate) struct AccessibilitySnapshot {
    pub(crate) elements: Vec<AccessibilityElement>,
    pub(crate) ids: Vec<i32>,
    last_id: i32,
    indices: HashMap<i32, usize>,
}

impl AccessibilitySnapshot {
    /// Moves on to `elements`, keeping the ids of controls it already held,
    /// and hands back what it replaced.
    pub(crate) fn update(
        &mut self,
        elements: Vec<AccessibilityElement>,
    ) -> Result<Replaced, AccessibilityIdentityError> {
        let mut previous: HashMap<_, _> = self
            .elements
            .iter()
            .zip(&self.ids)
            .enumerate()
            .map(|(index, (element, id))| (element.identity_key(), (*id, index)))
            .collect();
        let mut identities: HashMap<_, _> = HashMap::default();
        let mut indices = HashMap::default();
        let mut ids = Vec::with_capacity(elements.len());
        let mut was = Vec::with_capacity(elements.len());
        let mut last_id = self.last_id;
        for (index, element) in elements.iter().enumerate() {
            let identity = (element.node_id, element.canvas_key);
            if identities.insert(identity, ()).is_some() {
                return Err(AccessibilityIdentityError::Duplicate(
                    identity.0, identity.1,
                ));
            }
            let (id, old) = match previous.remove(&element.identity_key()) {
                Some((id, old)) => (id, Some(old)),
                None => {
                    last_id = last_id
                        .checked_add(1)
                        .ok_or(AccessibilityIdentityError::Exhausted)?;
                    (last_id, None)
                }
            };
            ids.push(id);
            was.push(old);
            indices.insert(id, index);
        }
        self.indices = indices;
        self.last_id = last_id;
        Ok(Replaced {
            elements: std::mem::replace(&mut self.elements, elements),
            ids: std::mem::replace(&mut self.ids, ids),
            was,
        })
    }

    pub(crate) fn element(&self, id: i32) -> Option<&AccessibilityElement> {
        self.elements.get(*self.indices.get(&id)?)
    }

    #[cfg(any(test, target_os = "android"))]
    pub(crate) fn identity(&self, id: i32) -> Option<(NodeId, Option<u64>)> {
        self.element(id)
            .map(|element| (element.node_id, element.canvas_key))
    }
}

#[cfg(test)]
#[path = "tests/accessibility_identity.rs"]
mod tests;
