use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

fn borrow_across_closure(cell: &RefCell<Vec<i32>>, on_change: impl Fn()) {
    let items = cell.borrow();
    on_change();
    let _ = items.len();
}

fn lock_across_dyn(lock: &Mutex<i32>, callback: &dyn Fn(i32)) {
    let value = lock.lock().unwrap();
    callback(0);
    let _ = *value;
}

fn borrow_ended_before(cell: &RefCell<Vec<i32>>, on_change: impl Fn()) {
    let items = cell.borrow();
    let count = items.len();
    drop(items);
    on_change();
    let _ = count;
}

fn copy_out(cell: &RefCell<Vec<i32>>, on_change: impl Fn()) {
    let count = cell.borrow().len();
    on_change();
    let _ = count;
}

fn named_function_is_fine(cell: &RefCell<Vec<i32>>) {
    let items = cell.borrow();
    helper(items.len());
}

fn helper(_: usize) {}

fn main() {
    let cell = RefCell::new(vec![1]);
    let lock = Mutex::new(1);
    borrow_across_closure(&cell, || {});
    lock_across_dyn(&lock, &|_| {});
    borrow_ended_before(&cell, || {});
    copy_out(&cell, || {});
    named_function_is_fine(&cell);
    lends_the_borrow(&cell, |items| items.len());
    lends_a_value_taken_from_it(&cell, |first| first.copied());
    let callback: Rc<dyn Fn()> = Rc::new(|| {});
    borrow_across_named_payload_drop(&cell, Box::new(1));
    borrow_across_named_callback(&cell, &callback);
    borrow_across_dyn_method(&cell, &QueueHost, Box::new(1));
    borrow_across_payload_drop(&cell, Box::new(1));
    borrow_across_listener_drop(&cell, Listener { callback: Box::new(|| {}) });
    borrow_across_closure_run_by_a_named_function(&cell, &callback);
    borrow_across_plain_named_functions(&cell);
    borrow_lent_to_a_named_function_that_drops_a_payload(&cell, Box::new(1));
    borrow_ended_by_an_inner_block(&cell, Box::new(1));
}

fn lends_the_borrow<R>(cell: &RefCell<Vec<i32>>, f: impl FnOnce(&[i32]) -> R) -> R {
    let items = cell.borrow();
    f(&items)
}

fn lends_a_value_taken_from_it<R>(cell: &RefCell<Vec<i32>>, f: impl FnOnce(Option<&i32>) -> R) -> R {
    let items = cell.borrow();
    let first = items.first();
    f(first)
}

trait Host {
    fn dispose(&self, payload: Box<dyn Any>);
}

struct QueueHost;

impl Host for QueueHost {
    fn dispose(&self, payload: Box<dyn Any>) {
        drop(payload);
    }
}

struct Listener {
    callback: Box<dyn Fn()>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        (self.callback)();
    }
}

fn drops_a_payload(payload: Box<dyn Any>) {
    drop(payload);
}

fn calls_a_stored_callback(callback: &Rc<dyn Fn()>) {
    callback();
}

fn run<F: FnOnce()>(f: F) {
    f();
}

fn pushes(values: &mut Vec<i32>) {
    values.push(1);
}

fn borrow_across_named_payload_drop(cell: &RefCell<Vec<i32>>, payload: Box<dyn Any>) {
    let items = cell.borrow();
    drops_a_payload(payload);
    let _ = items.len();
}

fn borrow_across_named_callback(cell: &RefCell<Vec<i32>>, callback: &Rc<dyn Fn()>) {
    let items = cell.borrow();
    calls_a_stored_callback(callback);
    let _ = items.len();
}

fn borrow_across_dyn_method(cell: &RefCell<Vec<i32>>, host: &dyn Host, payload: Box<dyn Any>) {
    let mut items = cell.borrow_mut();
    host.dispose(payload);
    items.push(1);
}

fn borrow_across_payload_drop(cell: &RefCell<Vec<i32>>, payload: Box<dyn Any>) {
    let items = cell.borrow();
    drop(payload);
    let _ = items.len();
}

fn borrow_across_listener_drop(cell: &RefCell<Vec<i32>>, listener: Listener) {
    let items = cell.borrow();
    drop(listener);
    let _ = items.len();
}

fn borrow_across_closure_run_by_a_named_function(cell: &RefCell<Vec<i32>>, callback: &Rc<dyn Fn()>) {
    let items = cell.borrow();
    run(|| callback());
    run(|| {});
    let _ = items.len();
}

fn borrow_across_plain_named_functions(cell: &RefCell<Vec<i32>>) {
    let mut items = cell.borrow_mut();
    pushes(&mut items);
    helper(items.len());
}

fn dispose_into(items: &mut Vec<i32>, host: &dyn Host, payload: Box<dyn Any>) {
    items.push(0);
    host.dispose(payload);
}

fn borrow_lent_to_a_named_function_that_drops_a_payload(cell: &RefCell<Vec<i32>>, payload: Box<dyn Any>) {
    let mut items = cell.borrow_mut();
    dispose_into(&mut items, &QueueHost, payload);
}

fn borrow_ended_by_an_inner_block(cell: &RefCell<Vec<i32>>, payload: Box<dyn Any>) {
    let count = {
        let items = cell.borrow();
        items.len()
    };
    drops_a_payload(payload);
    let _ = count;
}
