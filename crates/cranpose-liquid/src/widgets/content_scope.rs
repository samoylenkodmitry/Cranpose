use std::{cell::RefCell, rc::Rc};

pub(crate) struct ScopeContent<T> {
    items: Rc<RefCell<Vec<T>>>,
}

impl<T> ScopeContent<T> {
    pub(crate) fn collect<S>(build: impl FnOnce(Self) -> S, content: impl FnOnce(&S)) -> Vec<T> {
        let items = Rc::new(RefCell::new(Vec::new()));
        let scope = build(Self {
            items: Rc::clone(&items),
        });
        content(&scope);
        items.take()
    }

    pub(crate) fn push(&self, item: T) {
        self.items.borrow_mut().push(item);
    }
}

#[cfg(test)]
#[path = "tests/content_scope_tests.rs"]
mod tests;
