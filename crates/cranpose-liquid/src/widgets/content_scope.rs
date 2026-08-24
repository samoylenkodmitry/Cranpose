//! The seam every scoped Liquid widget declares its content through.
//!
//! A widget that takes content instead of a vector hands its caller a scope
//! whose calls append to one of these lists, then reads back what the call
//! built. Sharing the list means a scope type only has to state its own
//! vocabulary — `tab`, `segment`, `action` — and never the plumbing under it.

use std::{cell::RefCell, rc::Rc};

/// The list a scope appends its declarations to.
pub(crate) struct ScopeContent<T> {
    items: Rc<RefCell<Vec<T>>>,
}

impl<T> ScopeContent<T> {
    /// Runs `content` against the scope `build` wraps this list in, and
    /// returns what it declared, in the order it was declared.
    pub(crate) fn collect<S>(
        build: impl FnOnce(ScopeContent<T>) -> S,
        content: impl FnOnce(&S),
    ) -> Vec<T> {
        let items = Rc::new(RefCell::new(Vec::new()));
        let scope = build(ScopeContent {
            items: Rc::clone(&items),
        });
        content(&scope);
        items.take()
    }

    /// Adds one declaration to the end of the list.
    pub(crate) fn push(&self, item: T) {
        self.items.borrow_mut().push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Numbers {
        content: ScopeContent<u32>,
    }

    impl Numbers {
        fn number(&self, value: u32) {
            self.content.push(value);
        }
    }

    #[test]
    fn a_scope_keeps_the_order_its_content_declared() {
        let collected = ScopeContent::collect(
            |content| Numbers { content },
            |scope| {
                scope.number(3);
                scope.number(1);
                scope.number(2);
            },
        );
        assert_eq!(collected, vec![3, 1, 2]);
    }

    #[test]
    fn content_that_declares_nothing_collects_nothing() {
        let collected = ScopeContent::collect(|content| Numbers { content }, |_| {});
        assert!(collected.is_empty());
    }

    /// Two collections must not see each other's declarations: a widget
    /// recomposing builds its list from scratch every time.
    #[test]
    fn each_collection_starts_from_an_empty_list() {
        let build = |content| Numbers { content };
        let first = ScopeContent::collect(build, |scope| scope.number(7));
        let second = ScopeContent::collect(build, |scope| scope.number(8));
        assert_eq!(first, vec![7]);
        assert_eq!(second, vec![8]);
    }
}
