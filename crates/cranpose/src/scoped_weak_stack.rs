use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

pub(crate) struct ScopedWeakStack<T> {
    stack: RefCell<Vec<Weak<T>>>,
}

impl<T> ScopedWeakStack<T> {
    pub(crate) const fn new() -> Self {
        Self {
            stack: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn with_scope<R>(&self, value: &Rc<T>, f: impl FnOnce() -> R) -> R {
        struct PopGuard<'a, T>(&'a ScopedWeakStack<T>);

        impl<T> Drop for PopGuard<'_, T> {
            fn drop(&mut self) {
                self.0.stack.borrow_mut().pop();
            }
        }

        self.stack.borrow_mut().push(Rc::downgrade(value));
        let _guard = PopGuard(self);
        f()
    }

    pub(crate) fn current(&self) -> Option<Rc<T>> {
        let mut stack = self.stack.borrow_mut();
        loop {
            match stack.last().and_then(Weak::upgrade) {
                Some(value) => return Some(value),
                None if stack.is_empty() => return None,
                None => {
                    stack.pop();
                }
            }
        }
    }
}
