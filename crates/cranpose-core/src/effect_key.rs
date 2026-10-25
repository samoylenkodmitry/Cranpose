use std::any::Any;

/// The keys an effect last ran with, of any type.
pub(crate) struct EffectKey(Box<dyn Any>);

impl EffectKey {
    pub(crate) fn new<K: PartialEq + 'static>(key: K) -> Self {
        Self(Box::new(key))
    }

    /// Whether this holds keys of `key`'s type equal to it. A recomposition
    /// compares its keys here before it boxes them, which it does only when
    /// they changed.
    pub(crate) fn holds<K: PartialEq + 'static>(&self, key: &K) -> bool {
        self.0.downcast_ref::<K>().is_some_and(|held| held == key)
    }
}

#[cfg(test)]
#[path = "tests/effect_key_tests.rs"]
mod tests;
