use std::any::Any;

trait ErasedKeyValue: Any {
    fn as_any(&self) -> &dyn Any;
    fn eq_erased(&self, other: &dyn Any) -> bool;
}

impl<K: PartialEq + 'static> ErasedKeyValue for K {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn eq_erased(&self, other: &dyn Any) -> bool {
        match other.downcast_ref::<K>() {
            Some(other) => self == other,
            None => false,
        }
    }
}

pub(crate) struct EffectKey(Box<dyn ErasedKeyValue>);

impl EffectKey {
    pub(crate) fn new<K: PartialEq + 'static>(key: K) -> Self {
        EffectKey(Box::new(key))
    }

    pub(crate) fn differs_from(&self, previous: &EffectKey) -> bool {
        !self.0.eq_erased(previous.0.as_any())
    }
}

#[cfg(test)]
#[path = "tests/effect_key_tests.rs"]
mod tests;
