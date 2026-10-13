use std::ops::Deref;

pub(crate) struct ThreadState<T>(T);

impl<T> ThreadState<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for ThreadState<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> Drop for ThreadState<T> {
    fn drop(&mut self) {
        crate::runtime::forget_registered_runtimes();
    }
}
