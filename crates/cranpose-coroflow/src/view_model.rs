use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    hash::Hash,
    rc::Rc,
};

use coroflow::MainScope;
use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf, remember};

use crate::{
    dispatcher::require_main_dispatcher,
    hooks::{Handle, remember_shared},
};

type Models<K, VM> = HashMap<K, Rc<VM>>;
type Slots = HashMap<(TypeId, TypeId), Box<dyn Any>>;

/// The view models of one part of the UI — Android's `ViewModelStore`.
///
/// Every back stack entry of a `NavHost` owns one, and so does every
/// [`ViewModelStoreOwner`] block. [`viewModel`] finds its view model in the
/// nearest store, so the view model outlives the composable that asked for
/// it: a list item scrolled out of view gets the same view model back when it
/// returns. The view models live until the last clone of the store is
/// dropped, and dropping a view model cancels its `viewModelScope`.
///
/// Clones share the same view models.
#[derive(Clone, Default)]
pub struct ViewModelStore {
    models: Rc<RefCell<Slots>>,
}

impl PartialEq for ViewModelStore {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.models, &other.models)
    }
}

impl ViewModelStore {
    /// The view model of type `VM` stored under `key`, if there is one.
    pub fn get<K, VM>(&self, key: &K) -> Option<Rc<VM>>
    where
        K: Hash + Eq + 'static,
        VM: 'static,
    {
        self.models
            .borrow()
            .get(&slot::<K, VM>())
            .and_then(|models| models.downcast_ref::<Models<K, VM>>())
            .and_then(|models| models.get(key))
            .cloned()
    }

    /// Stores `model` under `key`, dropping the view model it replaces — how
    /// a preview or test hands a screen a view model built from fakes.
    pub fn put<K, VM>(&self, key: K, model: VM) -> Rc<VM>
    where
        K: Hash + Eq + 'static,
        VM: 'static,
    {
        let model = Rc::new(model);
        let replaced = self
            .models
            .borrow_mut()
            .entry(slot::<K, VM>())
            .or_insert_with(|| Box::new(Models::<K, VM>::new()))
            .downcast_mut::<Models<K, VM>>()
            .and_then(|models| models.insert(key, Rc::clone(&model)));
        drop(replaced);
        model
    }

    /// Drops every view model in the store — Android's
    /// `ViewModelStore.clear()`.
    pub fn clear(&self) {
        let models = std::mem::take(&mut *self.models.borrow_mut());
        drop(models);
    }

    fn get_or_create<K, VM>(&self, key: K, create: impl FnOnce() -> VM) -> Rc<VM>
    where
        K: Hash + Eq + 'static,
        VM: 'static,
    {
        match self.get(&key) {
            Some(model) => model,
            None => self.put(key, create()),
        }
    }
}

fn slot<K: 'static, VM: 'static>() -> (TypeId, TypeId) {
    (TypeId::of::<VM>(), TypeId::of::<K>())
}

fn local_view_model_store() -> CompositionLocal<Option<ViewModelStore>> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<Option<ViewModelStore>>>> =
            const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| None))
            .clone()
    })
}

/// Gives `content` its own [`ViewModelStore`], whose view models live as long
/// as this block stays in the composition — the arch starter's nested
/// `ScreenScope`.
///
/// Use it for a dialog, a sheet or any other self-contained part of the UI
/// whose business logic should end when the part closes.
#[track_caller]
pub fn ViewModelStoreOwner(content: impl FnOnce()) {
    let store = remember(ViewModelStore::default).with(ViewModelStore::clone);
    ProvideViewModelStore(store, content);
}

/// Makes `store` the store that [`viewModel`] calls inside `content` use —
/// Android's `LocalViewModelStoreOwner provides owner`.
#[track_caller]
pub fn ProvideViewModelStore(store: ViewModelStore, content: impl FnOnce()) {
    CompositionLocalProvider([local_view_model_store().provides(Some(store))], content);
}

/// The view model of type `VM` for `key` in the nearest [`ViewModelStore`],
/// built by `factory` the first time it is asked for — Android's
/// `viewModel(key) { }`.
///
/// `factory` receives the view model's [`MainScope`], its `viewModelScope`.
/// Pass `()` as the key when a store holds one view model of this type, and
/// something that tells them apart, such as an item id, when a list asks for
/// many. The view model lives as long as the store: until its back stack entry
/// is popped or its [`ViewModelStoreOwner`] leaves the composition. Outside
/// any store it lives as long as this position in the composition.
#[track_caller]
pub fn viewModel<K, VM>(key: K, factory: impl FnOnce(MainScope) -> VM) -> Handle<VM>
where
    K: Hash + Eq + 'static,
    VM: 'static,
{
    let create = || factory(MainScope::new(require_main_dispatcher("viewModel")));
    let Some(store) = local_view_model_store().current() else {
        return cranpose_core::key(0_usize, || remember_shared(|| Rc::new(create())));
    };
    let model = store.get_or_create(key, create);
    cranpose_core::key(Rc::as_ptr(&model).addr(), || remember_shared(|| model))
}
