use super::*;

#[test]
fn same_type_equal_values_do_not_differ() {
    let a = EffectKey::new(42i32);
    assert!(a.holds(&42i32));
}

#[test]
fn same_type_different_values_differ() {
    let a = EffectKey::new(1i32);
    assert!(!a.holds(&2i32));
}

#[test]
fn different_types_always_differ_regardless_of_direction() {
    let number = EffectKey::new(1i32);
    let text = EffectKey::new("1".to_string());
    assert!(!number.holds(&"1".to_string()));
    assert!(!text.holds(&1i32));
}

#[test]
fn tuple_keys_compare_field_by_field() {
    let a = EffectKey::new((1u32, "x".to_string()));
    assert!(a.holds(&(1u32, "x".to_string())));
    assert!(!a.holds(&(1u32, "y".to_string())));
}
