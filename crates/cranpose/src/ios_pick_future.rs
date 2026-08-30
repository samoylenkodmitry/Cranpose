#![allow(unsafe_code)]

use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, msg_send, rc::Retained};

pub(crate) struct PickSlot<T> {
    result: Option<T>,
    waker: Option<Waker>,
}

impl<T> Default for PickSlot<T> {
    fn default() -> Self {
        Self {
            result: None,
            waker: None,
        }
    }
}

pub(crate) type SharedPickSlot<T> = Rc<RefCell<PickSlot<T>>>;

pub(crate) struct PickFuture<T, D> {
    pub(crate) slot: SharedPickSlot<T>,
    pub(crate) _delegate: Retained<D>,
}

impl<T, D> Future for PickFuture<T, D> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<T> {
        let mut slot = self.slot.borrow_mut();
        if let Some(result) = slot.result.take() {
            Poll::Ready(result)
        } else {
            slot.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

pub(crate) fn new_pick_delegate<T, D>(slot: SharedPickSlot<T>, mtm: MainThreadMarker) -> Retained<D>
where
    D: DefinedClass<Ivars = SharedPickSlot<T>> + MainThreadOnly,
    D::Super: ClassType,
{
    let this = D::alloc(mtm).set_ivars(slot);
    unsafe { msg_send![super(this), init] }
}

pub(crate) fn resolve_pick_slot<T>(slot: &SharedPickSlot<T>, result: T) {
    let mut slot = slot.borrow_mut();
    slot.result = Some(result);
    if let Some(waker) = slot.waker.take() {
        waker.wake();
    }
}
