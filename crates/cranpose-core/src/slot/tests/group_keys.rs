use super::*;
use crate::collections::map::HashMap;

#[test]
fn group_keys_preserve_every_identity_component_at_numeric_boundaries() {
    let mut keys = HashMap::default();
    for static_key in [0, Key::MAX] {
        for ordinal in [0, u32::MAX] {
            for explicit in [None, Some(0), Some(1), Some(Key::MAX)] {
                let key = GroupKey::new(static_key, explicit, ordinal);
                assert_eq!(key.static_key, static_key);
                assert_eq!(key.ordinal, ordinal);
                assert_eq!(key.explicit_key(), explicit);
                assert!(keys.insert(key, (static_key, explicit, ordinal)).is_none());
            }
        }
    }
    assert_eq!(keys.len(), 16);
    for (key, &(static_key, explicit, ordinal)) in &keys {
        assert_eq!(
            keys.get(&GroupKey::new(static_key, explicit, ordinal)),
            Some(&(static_key, explicit, ordinal))
        );
        assert_eq!(key.explicit_key(), explicit);
    }
}

#[test]
fn movable_keys_preserve_zero_and_maximum_explicit_ids() {
    let movable = super::super::MOVABLE_STATIC_KEY;
    for id in [0, Key::MAX] {
        let key = GroupKey::new(movable, Some(id), 0);
        assert!(key.is_movable());
        assert_eq!(key.movable_id(), Some(id));
        let ordinary = GroupKey::new(0, Some(id), 0);
        assert!(!ordinary.is_movable());
        assert_eq!(ordinary.movable_id(), None);
    }
    let absent = GroupKey::new(movable, None, 0);
    assert!(!absent.is_movable());
    assert_eq!(absent.movable_id(), None);
}
