use std::{cell::RefCell, collections::BTreeMap};

use crate::{message::SourceBundle, preferences::CachedFormatter};

pub(crate) struct LocalizationThreadState {
    pub(crate) source_bundles: RefCell<BTreeMap<usize, SourceBundle>>,
    pub(crate) formatter: RefCell<Option<CachedFormatter>>,
}

impl LocalizationThreadState {
    const fn new() -> Self {
        Self {
            source_bundles: RefCell::new(BTreeMap::new()),
            formatter: RefCell::new(None),
        }
    }
}

thread_local! {
    static STATE: LocalizationThreadState = const { LocalizationThreadState::new() };
}

pub(crate) fn with<R>(f: impl FnOnce(&LocalizationThreadState) -> R) -> R {
    let mut f = Some(f);
    let mut call = |state: &LocalizationThreadState| f.take().map(|f| f(state));
    let ran = STATE.try_with(&mut call).ok().flatten();
    ran.or_else(|| call(&LocalizationThreadState::new()))
        .expect("the closure runs on exactly one state")
}
