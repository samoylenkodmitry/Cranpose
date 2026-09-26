use cranpose_core::{NodeId, collections::map::HashMap};

use crate::accessibility::AccessibilityElement;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum AccessibilityIdentityError {
    #[error("accessibility node IDs exhausted")]
    Exhausted,
    #[error("duplicate accessibility identity: node {0}, canvas key {1:?}")]
    Duplicate(NodeId, Option<u64>),
}

/// The elements and ids a snapshot held before an update replaced them.
pub(crate) struct ReplacedSnapshot {
    pub(crate) elements: Vec<AccessibilityElement>,
    pub(crate) ids: Vec<i32>,
}

#[derive(Default)]
pub(crate) struct AccessibilitySnapshot {
    pub(crate) elements: Vec<AccessibilityElement>,
    pub(crate) ids: Vec<i32>,
    last_id: i32,
    indices: HashMap<i32, usize>,
}

impl AccessibilitySnapshot {
    pub(crate) fn update(
        &mut self,
        elements: Vec<AccessibilityElement>,
    ) -> Result<ReplacedSnapshot, AccessibilityIdentityError> {
        let mut previous: HashMap<_, _> = self
            .elements
            .iter()
            .zip(&self.ids)
            .map(|(element, id)| (element.identity_key(), *id))
            .collect();
        let mut identities: HashMap<_, _> = HashMap::default();
        let mut indices = HashMap::default();
        let mut ids = Vec::with_capacity(elements.len());
        let mut last_id = self.last_id;
        for (index, element) in elements.iter().enumerate() {
            let identity = (element.node_id, element.canvas_key);
            if identities.insert(identity, ()).is_some() {
                return Err(AccessibilityIdentityError::Duplicate(
                    identity.0, identity.1,
                ));
            }
            let id = match previous.remove(&element.identity_key()) {
                Some(id) => id,
                None => {
                    last_id = last_id
                        .checked_add(1)
                        .ok_or(AccessibilityIdentityError::Exhausted)?;
                    last_id
                }
            };
            ids.push(id);
            indices.insert(id, index);
        }
        self.indices = indices;
        self.last_id = last_id;
        Ok(ReplacedSnapshot {
            elements: std::mem::replace(&mut self.elements, elements),
            ids: std::mem::replace(&mut self.ids, ids),
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
