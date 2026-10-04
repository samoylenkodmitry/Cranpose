use std::{cell::RefCell, rc::Rc};

use cranpose_core::{CompositionLocalProvider, rememberMutableStateOf};
use cranpose_ui::Locale;

use crate::system_languages::{local_system_languages, system_languages};

pub(super) struct SystemLanguagesState(RefCell<Rc<[Locale]>>);

impl Default for SystemLanguagesState {
    fn default() -> Self {
        Self(RefCell::new(system_languages().into()))
    }
}

impl SystemLanguagesState {
    pub(super) fn refresh(&self) -> bool {
        let next = system_languages();
        let mut current = self.0.borrow_mut();
        if current.as_ref() == next.as_slice() {
            return false;
        }
        *current = next.into();
        true
    }
}

#[cranpose_ui::composable(no_skip)]
pub(super) fn ProvideSystemLanguages(state: Rc<SystemLanguagesState>, content: impl FnOnce()) {
    let revision = rememberMutableStateOf(|| 0u64);
    let _current_revision = revision.get();
    let refresh = state.clone();
    crate::LifecycleEffect((), move |event| {
        if event.to == cranpose_services::LifecycleState::Resumed && refresh.refresh() {
            revision.update(|revision| *revision = revision.wrapping_add(1));
        }
    });
    let languages = state.0.borrow().clone();
    CompositionLocalProvider([local_system_languages().provides(languages)], content);
}
