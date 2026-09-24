use super::*;

fn empty_ancestor_lookup(_: &ModifierLocalToken) -> Option<ResolvedModifierLocal> {
    None
}

#[test]
fn modifier_local_keys_do_not_use_process_global_counter() {
    let source = include_str!("../local.rs");
    assert!(!source.contains(concat!("NEXT_", "MODIFIER_LOCAL_ID")));
    assert!(!source.contains(concat!("Atomic", "U64")));
}

#[test]
fn modifier_local_key_identity_is_retained_per_key() {
    let first = ModifierLocalKey::new(|| 1_i32);
    let first_clone = first.clone();
    let second = ModifierLocalKey::new(|| 1_i32);

    assert_eq!(first.token(), first_clone.token());
    assert_ne!(first.token(), second.token());
}

#[test]
fn modifier_local_provider_type_mismatch_falls_back_to_default() {
    let key = ModifierLocalKey::new(|| 7_i32);
    let mut providers = HashMap::new();
    providers.insert(
        key.token().id(),
        ProviderRecord::new(Rc::new(String::from("wrong")) as Rc<dyn Any>, 4),
    );
    let mut dependencies = Vec::new();
    let mut ancestor_lookup = empty_ancestor_lookup;
    let mut scope =
        ModifierLocalReadScope::new(&providers, &mut ancestor_lookup, &mut dependencies);

    assert_eq!(*scope.get(&key), 7);
    assert!(matches!(dependencies[0].source, DependencySource::Chain));
    assert_eq!(dependencies[0].version, 4);
}

#[test]
fn modifier_local_ancestor_type_mismatch_falls_back_to_default() {
    let key = ModifierLocalKey::new(|| 11_i32);
    let providers = HashMap::new();
    let mut dependencies = Vec::new();
    let mut ancestor_lookup = |token: &ModifierLocalToken| {
        if *token == key.token() {
            Some(ResolvedModifierLocal::new(
                Rc::new(String::from("wrong")) as Rc<dyn Any>,
                9,
                ModifierLocalSource::Ancestor,
            ))
        } else {
            None
        }
    };
    let mut scope =
        ModifierLocalReadScope::new(&providers, &mut ancestor_lookup, &mut dependencies);

    assert_eq!(*scope.get(&key), 11);
    assert!(matches!(dependencies[0].source, DependencySource::Ancestor));
    assert_eq!(dependencies[0].version, 9);
}

#[test]
fn modifier_local_fallback_cache_reconciles_mismatched_cached_value() {
    let key = ModifierLocalKey::new(|| 23_i32);
    let providers = HashMap::new();
    let mut dependencies = Vec::new();
    let mut ancestor_lookup = empty_ancestor_lookup;
    let cache_is_typed = {
        let mut scope =
            ModifierLocalReadScope::new(&providers, &mut ancestor_lookup, &mut dependencies);
        scope.fallbacks.insert(
            key.token().id(),
            Rc::new(String::from("wrong")) as Rc<dyn Any>,
        );

        assert_eq!(*scope.get(&key), 23);
        assert_eq!(*scope.get(&key), 23);
        scope
            .fallbacks
            .get(&key.token().id())
            .and_then(|value| value.downcast_ref::<i32>())
            .is_some()
    };

    assert_eq!(dependencies.len(), 2);
    assert!(cache_is_typed);
}
