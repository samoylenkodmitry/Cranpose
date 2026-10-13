use std::{cell::RefCell, collections::BTreeMap};

use crate::{message::SourceBundle, preferences::CachedFormatter};

pub(crate) struct LocalizationThreadState {
    pub(crate) source_bundles: RefCell<BTreeMap<usize, SourceBundle>>,
    pub(crate) formatter: RefCell<Option<CachedFormatter>>,
}

thread_local! {
    static STATE: LocalizationThreadState = const {
        LocalizationThreadState {
            source_bundles: RefCell::new(BTreeMap::new()),
            formatter: RefCell::new(None),
        }
    };
}

pub(crate) fn with<R>(f: impl FnOnce(&LocalizationThreadState) -> R) -> R {
    STATE.with(f)
}
