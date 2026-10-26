use std::cell::RefCell;
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
