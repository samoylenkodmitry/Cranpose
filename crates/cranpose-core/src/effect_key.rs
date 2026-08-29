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
mod tests {
    use super::*;

    #[test]
    fn same_type_equal_values_do_not_differ() {
        let a = EffectKey::new(42i32);
        let b = EffectKey::new(42i32);
        assert!(!a.differs_from(&b));
    }

    #[test]
    fn same_type_different_values_differ() {
        let a = EffectKey::new(1i32);
        let b = EffectKey::new(2i32);
        assert!(a.differs_from(&b));
    }

    #[test]
    fn different_types_always_differ_regardless_of_direction() {
        let number = EffectKey::new(1i32);
        let text = EffectKey::new("1".to_string());
        assert!(number.differs_from(&text));
        assert!(text.differs_from(&number));
    }

    #[test]
    fn tuple_keys_compare_field_by_field() {
        let a = EffectKey::new((1u32, "x".to_string()));
        let b = EffectKey::new((1u32, "x".to_string()));
        let c = EffectKey::new((1u32, "y".to_string()));
        assert!(!a.differs_from(&b));
        assert!(a.differs_from(&c));
    }
}
