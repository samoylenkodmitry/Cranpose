use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierInvalidation, ModifierNode, ModifierNodeChain,
    ModifierNodeElement, NodeCapabilities, NodeState,
};

#[derive(Clone)]
struct ModifierLocalId(Rc<()>);

impl ModifierLocalId {
    fn new() -> Self {
        Self(Rc::new(()))
    }
}

impl fmt::Debug for ModifierLocalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ModifierLocalId")
            .field(&(Rc::as_ptr(&self.0) as usize))
            .finish()
    }
}

impl PartialEq for ModifierLocalId {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ModifierLocalId {}

impl Hash for ModifierLocalId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ModifierLocalToken {
    id: ModifierLocalId,
    type_id: TypeId,
}

impl ModifierLocalToken {
    fn new(type_id: TypeId) -> Self {
        let id = ModifierLocalId::new();
        Self { id, type_id }
    }

    fn id(&self) -> ModifierLocalId {
        self.id.clone()
    }
}

/// Type-safe key referencing a modifier local value.
#[derive(Clone)]
pub struct ModifierLocalKey<T: 'static> {
    token: ModifierLocalToken,
    default: Rc<dyn Fn() -> T>,
}

impl<T: 'static> ModifierLocalKey<T> {
    pub fn new(factory: impl Fn() -> T + 'static) -> Self {
        Self {
            token: ModifierLocalToken::new(TypeId::of::<T>()),
            default: Rc::new(factory),
        }
    }

    pub(crate) fn token(&self) -> ModifierLocalToken {
        self.token.clone()
    }

    pub(crate) fn default_value(&self) -> T {
        (self.default)()
    }
}

impl<T: 'static> PartialEq for ModifierLocalKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.token == other.token
    }
}

impl<T: 'static> Eq for ModifierLocalKey<T> {}

impl<T: 'static> Hash for ModifierLocalKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.token.hash(state);
    }
}

/// Node responsible for providing a modifier local value.
pub struct ModifierLocalProviderNode {
    token: ModifierLocalToken,
    value_factory: Rc<dyn Fn() -> Box<dyn Any>>,
    value: Rc<dyn Any>,
    version: u64,
    state: NodeState,
}

impl ModifierLocalProviderNode {
    fn new(token: ModifierLocalToken, factory: Rc<dyn Fn() -> Box<dyn Any>>) -> Self {
        Self {
            token,
            value: Self::create_value(&factory),
            value_factory: factory,
            version: 0,
            state: NodeState::new(),
        }
    }

    fn update_value(&mut self) {
        self.value = Self::create_value(&self.value_factory);
        self.version = self.version.wrapping_add(1);
    }

    fn set_factory(&mut self, factory: Rc<dyn Fn() -> Box<dyn Any>>) {
        self.value_factory = factory;
        self.update_value();
    }

    fn token(&self) -> ModifierLocalToken {
        self.token.clone()
    }

    fn value(&self) -> Rc<dyn Any> {
        self.value.clone()
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn create_value(factory: &Rc<dyn Fn() -> Box<dyn Any>>) -> Rc<dyn Any> {
        Rc::from(factory())
    }
}

impl DelegatableNode for ModifierLocalProviderNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ModifierLocalProviderNode {}

/// Node responsible for observing modifier local changes.
pub struct ModifierLocalConsumerNode {
    callback: Rc<dyn for<'a> Fn(&mut ModifierLocalReadScope<'a>)>,
    state: NodeState,
}

impl ModifierLocalConsumerNode {
    fn new(callback: Rc<dyn for<'a> Fn(&mut ModifierLocalReadScope<'a>)>) -> Self {
        Self {
            callback,
            state: NodeState::new(),
        }
    }

    fn notify(&self, scope: &mut ModifierLocalReadScope<'_>) {
        (self.callback)(scope);
    }

    fn id(&self) -> usize {
        self as *const Self as usize
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedModifierLocal {
    value: Rc<dyn Any>,
    version: u64,
    source: ModifierLocalSource,
}

impl ResolvedModifierLocal {
    fn new(value: Rc<dyn Any>, version: u64, source: ModifierLocalSource) -> Self {
        Self {
            value,
            version,
            source,
        }
    }

    pub(crate) fn value(&self) -> Rc<dyn Any> {
        self.value.clone()
    }

    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    pub(crate) fn with_source(mut self, source: ModifierLocalSource) -> Self {
        self.source = source;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModifierLocalSource {
    Chain,
    Ancestor,
}

pub(crate) type ModifierLocalAncestorResolver<'a> =
    dyn FnMut(&ModifierLocalToken) -> Option<ResolvedModifierLocal> + 'a;

#[derive(Clone)]
struct ProviderRecord {
    value: Rc<dyn Any>,
    version: u64,
}

impl ProviderRecord {
    fn new(value: Rc<dyn Any>, version: u64) -> Self {
        Self { value, version }
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn value(&self) -> &Rc<dyn Any> {
        &self.value
    }
}

#[derive(Clone)]
struct DependencyRecord {
    token: ModifierLocalToken,
    source: DependencySource,
    version: u64,
}

impl DependencyRecord {
    fn from_chain(token: ModifierLocalToken, version: u64) -> Self {
        Self {
            token,
            source: DependencySource::Chain,
            version,
        }
    }

    fn from_ancestor(token: ModifierLocalToken, version: u64) -> Self {
        Self {
            token,
            source: DependencySource::Ancestor,
            version,
        }
    }

    fn from_default(token: ModifierLocalToken) -> Self {
        Self {
            token,
            source: DependencySource::Default,
            version: 0,
        }
    }

    fn is_dirty(
        &self,
        providers: &HashMap<ModifierLocalId, ProviderRecord>,
        ancestor_lookup: &mut ModifierLocalAncestorResolver<'_>,
    ) -> bool {
        match self.source {
            DependencySource::Chain => match providers.get(&self.token.id()) {
                Some(record) => record.version() != self.version,
                None => true,
            },
            DependencySource::Ancestor => {
                if providers.contains_key(&self.token.id()) {
                    return true;
                }
                ancestor_lookup(&self.token)
                    .map(|resolved| resolved.version() != self.version)
                    .unwrap_or(true)
            }
            DependencySource::Default => {
                providers.contains_key(&self.token.id()) || ancestor_lookup(&self.token).is_some()
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DependencySource {
    Chain,
    Ancestor,
    Default,
}

struct ConsumerState {
    dependencies: Vec<DependencyRecord>,
}

impl ConsumerState {
    fn new(dependencies: Vec<DependencyRecord>) -> Self {
        Self { dependencies }
    }

    fn needs_update(
        &self,
        providers: &HashMap<ModifierLocalId, ProviderRecord>,
        ancestor_lookup: &mut ModifierLocalAncestorResolver<'_>,
    ) -> bool {
        if self.dependencies.is_empty() {
            return true;
        }
        self.dependencies
            .iter()
            .any(|dependency| dependency.is_dirty(providers, ancestor_lookup))
    }
}

impl DelegatableNode for ModifierLocalConsumerNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ModifierLocalConsumerNode {}

#[derive(Clone)]
pub struct ModifierLocalProviderElement {
    token: ModifierLocalToken,
    factory: Rc<dyn Fn() -> Box<dyn Any>>,
}

impl ModifierLocalProviderElement {
    pub fn new<T, F>(key: ModifierLocalKey<T>, factory: F) -> Self
    where
        T: 'static,
        F: Fn() -> T + 'static,
    {
        let erased = Rc::new(move || -> Box<dyn Any> { Box::new(factory()) });
        Self {
            token: key.token(),
            factory: erased,
        }
    }
}

impl fmt::Debug for ModifierLocalProviderElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModifierLocalProviderElement")
            .field("id", &self.token.id())
            .finish()
    }
}

impl PartialEq for ModifierLocalProviderElement {
    fn eq(&self, other: &Self) -> bool {
        // Type-based matching: compare only tokens, not factory closures
        // Nodes are updated via update() method, preserving behavior
        self.token == other.token
    }
}

impl Eq for ModifierLocalProviderElement {}

impl Hash for ModifierLocalProviderElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Consistent hash based on token only
        "modifier_local_provider".hash(state);
        self.token.hash(state);
    }
}

impl ModifierNodeElement for ModifierLocalProviderElement {
    type Node = ModifierLocalProviderNode;

    fn create(&self) -> Self::Node {
        ModifierLocalProviderNode::new(self.token.clone(), self.factory.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.set_factory(self.factory.clone());
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::MODIFIER_LOCALS
    }

    fn always_update(&self) -> bool {
        // Factory closure might change even if token is same
        true
    }
}

#[derive(Clone)]
pub struct ModifierLocalConsumerElement {
    callback: Rc<dyn for<'a> Fn(&mut ModifierLocalReadScope<'a>)>,
}

impl ModifierLocalConsumerElement {
    pub fn new<F>(callback: F) -> Self
    where
        F: for<'a> Fn(&mut ModifierLocalReadScope<'a>) + 'static,
    {
        Self {
            callback: Rc::new(callback),
        }
    }
}

impl fmt::Debug for ModifierLocalConsumerElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ModifierLocalConsumerElement")
    }
}

impl PartialEq for ModifierLocalConsumerElement {
    fn eq(&self, _other: &Self) -> bool {
        // Type-based matching: always equal for same type
        // Nodes are updated via update() method, preserving behavior
        true
    }
}

impl Eq for ModifierLocalConsumerElement {}

impl Hash for ModifierLocalConsumerElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Consistent hash for type-based matching
        "modifier_local_consumer".hash(state);
    }
}

impl ModifierNodeElement for ModifierLocalConsumerElement {
    type Node = ModifierLocalConsumerNode;

    fn create(&self) -> Self::Node {
        ModifierLocalConsumerNode::new(self.callback.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.callback = self.callback.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::MODIFIER_LOCALS
    }

    fn always_update(&self) -> bool {
        // Callback closure might change
        true
    }
}

/// Lightweight read scope surfaced to modifier local consumers.
pub struct ModifierLocalReadScope<'a> {
    providers: &'a HashMap<ModifierLocalId, ProviderRecord>,
    ancestor_lookup: &'a mut ModifierLocalAncestorResolver<'a>,
    dependencies: &'a mut Vec<DependencyRecord>,
    fallbacks: HashMap<ModifierLocalId, Rc<dyn Any>>,
}

impl<'a> ModifierLocalReadScope<'a> {
    fn new(
        providers: &'a HashMap<ModifierLocalId, ProviderRecord>,
        ancestor_lookup: &'a mut ModifierLocalAncestorResolver<'a>,
        dependencies: &'a mut Vec<DependencyRecord>,
    ) -> Self {
        Self {
            providers,
            ancestor_lookup,
            dependencies,
            fallbacks: HashMap::new(),
        }
    }

    pub fn get<T: 'static>(&mut self, key: &ModifierLocalKey<T>) -> &T {
        let token = key.token();
        if let Some(record) = self.providers.get(&token.id()) {
            self.dependencies
                .push(DependencyRecord::from_chain(token, record.version()));
            if let Some(value) = record.value().downcast_ref::<T>() {
                return value;
            }
            return self.default_value_for(key);
        }

        if let Some(resolved) = (self.ancestor_lookup)(&token) {
            self.dependencies.push(DependencyRecord::from_ancestor(
                token.clone(),
                resolved.version(),
            ));
            let resolved_value = resolved.value();
            if resolved_value.downcast_ref::<T>().is_some() {
                self.fallbacks.insert(token.id(), resolved_value);
                return self.fallback_value_for(key);
            }
            return self.default_value_for(key);
        }

        self.dependencies
            .push(DependencyRecord::from_default(token));
        self.default_value_for(key)
    }

    fn fallback_value_for<T: 'static>(&mut self, key: &ModifierLocalKey<T>) -> &T {
        let id = key.token().id();
        if self
            .fallbacks
            .get(&id)
            .and_then(|value| value.downcast_ref::<T>())
            .is_none()
        {
            self.fallbacks
                .insert(id.clone(), Rc::new(key.default_value()) as Rc<dyn Any>);
        }

        match self
            .fallbacks
            .get(&id)
            .and_then(|value| value.downcast_ref::<T>())
        {
            Some(value) => value,
            None => Box::leak(Box::new(key.default_value())),
        }
    }

    fn default_value_for<T: 'static>(&mut self, key: &ModifierLocalKey<T>) -> &T {
        self.fallback_value_for(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ancestor_lookup(_: &ModifierLocalToken) -> Option<ResolvedModifierLocal> {
        None
    }

    #[test]
    fn modifier_local_keys_do_not_use_process_global_counter() {
        let source = include_str!("local.rs");
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
}

#[derive(Default)]
pub struct ModifierLocalManager {
    providers: HashMap<ModifierLocalId, ProviderRecord>,
    consumers: HashMap<usize, ConsumerState>,
}

impl ModifierLocalManager {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(private_interfaces)]
    pub fn sync(
        &mut self,
        chain: &ModifierNodeChain,
        ancestor_lookup: &mut ModifierLocalAncestorResolver<'_>,
    ) -> Vec<ModifierInvalidation> {
        if !chain.has_capability(NodeCapabilities::MODIFIER_LOCALS) {
            self.providers.clear();
            self.consumers.clear();
            return Vec::new();
        }

        let mut providers: HashMap<ModifierLocalId, ProviderRecord> = HashMap::new();
        let mut seen_consumers = HashSet::new();
        let mut invalidations = Vec::new();

        chain.for_each_node_with_capability(NodeCapabilities::MODIFIER_LOCALS, |_ref, node| {
            if let Some(provider) = node.as_any().downcast_ref::<ModifierLocalProviderNode>() {
                providers.insert(
                    provider.token().id(),
                    ProviderRecord::new(provider.value(), provider.version()),
                );
                return;
            }

            if let Some(consumer) = node.as_any().downcast_ref::<ModifierLocalConsumerNode>() {
                let id = consumer.id();
                seen_consumers.insert(id);
                let needs_update = self
                    .consumers
                    .get(&id)
                    .map(|state| state.needs_update(&providers, ancestor_lookup))
                    .unwrap_or(true);
                if !needs_update {
                    return;
                }

                let mut dependencies = Vec::new();
                {
                    let mut scope =
                        ModifierLocalReadScope::new(&providers, ancestor_lookup, &mut dependencies);
                    consumer.notify(&mut scope);
                }
                self.consumers.insert(id, ConsumerState::new(dependencies));
                if !invalidations
                    .iter()
                    .any(|entry: &ModifierInvalidation| entry.kind() == InvalidationKind::Layout)
                {
                    invalidations.push(ModifierInvalidation::new(
                        InvalidationKind::Layout,
                        NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS,
                    ));
                }
            }
        });

        self.providers = providers;
        self.consumers.retain(|id, _| seen_consumers.contains(id));

        invalidations
    }

    pub(crate) fn resolve(&self, token: &ModifierLocalToken) -> Option<ResolvedModifierLocal> {
        self.providers.get(&token.id()).map(|record| {
            ResolvedModifierLocal::new(
                record.value().clone(),
                record.version(),
                ModifierLocalSource::Chain,
            )
        })
    }
}
