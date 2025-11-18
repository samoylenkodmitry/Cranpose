use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use compose_foundation::{
    DelegatableNode, InvalidationKind, ModifierInvalidation, ModifierNode, ModifierNodeChain,
    ModifierNodeElement, NodeCapabilities, NodeState,
};

/// Unique identifier generator for modifier local keys.
static NEXT_MODIFIER_LOCAL_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ModifierLocalId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ModifierLocalToken {
    id: ModifierLocalId,
    type_id: TypeId,
}

impl ModifierLocalToken {
    fn new(type_id: TypeId) -> Self {
        let id = ModifierLocalId(NEXT_MODIFIER_LOCAL_ID.fetch_add(1, Ordering::Relaxed));
        Self { id, type_id }
    }

    fn id(&self) -> ModifierLocalId {
        self.id
    }

    fn type_id(&self) -> TypeId {
        self.type_id
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
        self.token
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

/// Creates a new modifier local key with the provided default factory.
pub fn modifier_local_of<T: 'static>(factory: impl Fn() -> T + 'static) -> ModifierLocalKey<T> {
    ModifierLocalKey::new(factory)
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
        self.token
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

    pub(crate) fn source(&self) -> ModifierLocalSource {
        self.source
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
    dyn FnMut(ModifierLocalToken) -> Option<ResolvedModifierLocal> + 'a;

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
                ancestor_lookup(self.token)
                    .map(|resolved| resolved.version() != self.version)
                    .unwrap_or(true)
            }
            DependencySource::Default => {
                providers.contains_key(&self.token.id()) || ancestor_lookup(self.token).is_some()
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

    fn update_dependencies(&mut self, dependencies: Vec<DependencyRecord>) {
        self.dependencies = dependencies;
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
        self.token == other.token && Rc::ptr_eq(&self.factory, &other.factory)
    }
}

impl Eq for ModifierLocalProviderElement {}

impl Hash for ModifierLocalProviderElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.token.hash(state);
        Rc::as_ptr(&self.factory).hash(state);
    }
}

impl ModifierNodeElement for ModifierLocalProviderElement {
    type Node = ModifierLocalProviderNode;

    fn create(&self) -> Self::Node {
        ModifierLocalProviderNode::new(self.token, self.factory.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.set_factory(self.factory.clone());
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::MODIFIER_LOCALS
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
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.callback, &other.callback)
    }
}

impl Eq for ModifierLocalConsumerElement {}

impl Hash for ModifierLocalConsumerElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.callback).hash(state);
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
            return record
                .value()
                .downcast_ref::<T>()
                .expect("modifier local type mismatch");
        }

        if let Some(resolved) = (self.ancestor_lookup)(token) {
            self.dependencies
                .push(DependencyRecord::from_ancestor(token, resolved.version()));
            let entry = self
                .fallbacks
                .entry(token.id())
                .or_insert_with(|| resolved.value());
            return entry
                .downcast_ref::<T>()
                .expect("modifier local type mismatch");
        }

        self.dependencies
            .push(DependencyRecord::from_default(token));
        let value = self
            .fallbacks
            .entry(token.id())
            .or_insert_with(|| Rc::new(key.default_value()) as Rc<dyn Any>);
        value
            .downcast_ref::<T>()
            .expect("modifier local default type mismatch")
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

    pub(crate) fn resolve(&self, token: ModifierLocalToken) -> Option<ResolvedModifierLocal> {
        self.providers.get(&token.id()).map(|record| {
            ResolvedModifierLocal::new(
                record.value().clone(),
                record.version(),
                ModifierLocalSource::Chain,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_foundation::{modifier_element, BasicModifierNodeContext, ModifierNodeChain};
    use std::cell::RefCell;

    #[derive(Clone)]
    struct DelegatingProviderElement {
        token: ModifierLocalToken,
        factory: Rc<dyn Fn() -> Box<dyn Any>>,
    }

    impl DelegatingProviderElement {
        fn new<T: 'static + Clone>(key: ModifierLocalKey<T>, value: T) -> Self {
            let stored = Rc::new(value);
            let factory = Rc::new({
                let stored = stored.clone();
                move || -> Box<dyn Any> { Box::new((stored.as_ref()).clone()) }
            });
            Self {
                token: key.token(),
                factory,
            }
        }
    }

    impl fmt::Debug for DelegatingProviderElement {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("DelegatingProviderElement")
        }
    }

    impl PartialEq for DelegatingProviderElement {
        fn eq(&self, other: &Self) -> bool {
            self.token == other.token && Rc::ptr_eq(&self.factory, &other.factory)
        }
    }

    impl Eq for DelegatingProviderElement {}

    impl Hash for DelegatingProviderElement {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.token.hash(state);
            Rc::as_ptr(&self.factory).hash(state);
        }
    }

    struct DelegatingProviderNode {
        state: NodeState,
        delegate: ModifierLocalProviderNode,
    }

    impl DelegatingProviderNode {
        fn new(token: ModifierLocalToken, factory: Rc<dyn Fn() -> Box<dyn Any>>) -> Self {
            let mut node = Self {
                state: NodeState::new(),
                delegate: ModifierLocalProviderNode::new(token, factory),
            };
            node.state
                .set_capabilities(NodeCapabilities::MODIFIER_LOCALS);
            node.delegate
                .node_state()
                .set_capabilities(NodeCapabilities::MODIFIER_LOCALS);
            node
        }
    }

    impl DelegatableNode for DelegatingProviderNode {
        fn node_state(&self) -> &NodeState {
            &self.state
        }
    }

    impl ModifierNode for DelegatingProviderNode {
        fn for_each_delegate<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn ModifierNode)) {
            visitor(&self.delegate);
        }

        fn for_each_delegate_mut<'a>(
            &'a mut self,
            visitor: &mut dyn FnMut(&'a mut dyn ModifierNode),
        ) {
            visitor(&mut self.delegate);
        }
    }

    #[derive(Clone)]
    struct DelegatingConsumerElement {
        callback: Rc<dyn for<'a> Fn(&mut ModifierLocalReadScope<'a>)>,
    }

    impl DelegatingConsumerElement {
        fn new(callback: Rc<dyn for<'a> Fn(&mut ModifierLocalReadScope<'a>)>) -> Self {
            Self { callback }
        }
    }

    impl fmt::Debug for DelegatingConsumerElement {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("DelegatingConsumerElement")
        }
    }

    impl PartialEq for DelegatingConsumerElement {
        fn eq(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.callback, &other.callback)
        }
    }

    impl Eq for DelegatingConsumerElement {}

    impl Hash for DelegatingConsumerElement {
        fn hash<H: Hasher>(&self, state: &mut H) {
            Rc::as_ptr(&self.callback).hash(state);
        }
    }

    struct DelegatingConsumerNode {
        state: NodeState,
        delegate: ModifierLocalConsumerNode,
    }

    impl DelegatingConsumerNode {
        fn new(callback: Rc<dyn for<'a> Fn(&mut ModifierLocalReadScope<'a>)>) -> Self {
            let mut node = Self {
                state: NodeState::new(),
                delegate: ModifierLocalConsumerNode::new(callback),
            };
            node.state
                .set_capabilities(NodeCapabilities::MODIFIER_LOCALS);
            node.delegate
                .node_state()
                .set_capabilities(NodeCapabilities::MODIFIER_LOCALS);
            node
        }
    }

    impl DelegatableNode for DelegatingConsumerNode {
        fn node_state(&self) -> &NodeState {
            &self.state
        }
    }

    impl ModifierNode for DelegatingConsumerNode {
        fn for_each_delegate<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn ModifierNode)) {
            visitor(&self.delegate);
        }

        fn for_each_delegate_mut<'a>(
            &'a mut self,
            visitor: &mut dyn FnMut(&'a mut dyn ModifierNode),
        ) {
            visitor(&mut self.delegate);
        }
    }

    impl ModifierNodeElement for DelegatingProviderElement {
        type Node = DelegatingProviderNode;

        fn create(&self) -> Self::Node {
            DelegatingProviderNode::new(self.token, self.factory.clone())
        }

        fn update(&self, node: &mut Self::Node) {
            node.delegate.set_factory(self.factory.clone());
        }

        fn capabilities(&self) -> NodeCapabilities {
            NodeCapabilities::MODIFIER_LOCALS
        }
    }

    impl ModifierNodeElement for DelegatingConsumerElement {
        type Node = DelegatingConsumerNode;

        fn create(&self) -> Self::Node {
            DelegatingConsumerNode::new(self.callback.clone())
        }

        fn update(&self, node: &mut Self::Node) {
            node.delegate.callback = self.callback.clone();
        }

        fn capabilities(&self) -> NodeCapabilities {
            NodeCapabilities::MODIFIER_LOCALS
        }
    }

    #[test]
    fn modifier_local_manager_visits_delegated_nodes() {
        let key = modifier_local_of(|| 0i32);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let key_for_consumer = key.clone();
        let consumer_callback: Rc<dyn for<'a> Fn(&mut ModifierLocalReadScope<'a>)> = {
            let observed = observed.clone();
            Rc::new(move |scope| {
                observed.borrow_mut().push(*scope.get(&key_for_consumer));
            })
        };

        let elements = vec![
            modifier_element(DelegatingProviderElement::new(key.clone(), 42)),
            modifier_element(DelegatingConsumerElement::new(consumer_callback)),
        ];

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        chain.update_from_slice(&elements, &mut context);

        let mut manager = ModifierLocalManager::new();
        let mut ancestor_lookup = |_token: ModifierLocalToken| None;
        let invalidations = manager.sync(&chain, &mut ancestor_lookup);
        assert_eq!(
            invalidations,
            vec![ModifierInvalidation::new(
                InvalidationKind::Layout,
                NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS
            )]
        );
        assert_eq!(observed.borrow().as_slice(), &[42]);

        let resolved = manager
            .resolve(key.token())
            .expect("provider should be registered");
        assert_eq!(
            *resolved
                .value()
                .downcast_ref::<i32>()
                .expect("modifier local type mismatch"),
            42
        );
        assert_eq!(resolved.source(), ModifierLocalSource::Chain);
    }
}
