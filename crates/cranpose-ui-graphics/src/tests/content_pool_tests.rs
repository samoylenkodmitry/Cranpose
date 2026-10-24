use std::rc::Rc;

use super::ContentPool;

fn shared(pool: &mut ContentPool<Rc<str>>, hash: u64, text: &str) -> Rc<str> {
    pool.with(hash, |value| &**value == text, || Rc::from(text), Rc::clone)
}

#[test]
fn a_held_value_is_shared_while_others_come_and_go() {
    let mut pool = ContentPool::new();
    let label = shared(&mut pool, 1, "Score");
    for frame in 0..1000u64 {
        let reading = shared(&mut pool, 100 + frame, &format!("Reading {frame}"));
        drop(reading);
        assert!(Rc::ptr_eq(&label, &shared(&mut pool, 1, "Score")));
    }
}

#[test]
fn values_nothing_holds_leave() {
    let mut pool = ContentPool::new();
    let held: Vec<Rc<str>> = (0..40)
        .map(|index| shared(&mut pool, index, &format!("Label {index}")))
        .collect();
    for frame in 0..10_000u64 {
        drop(shared(&mut pool, 1000 + frame, &format!("Reading {frame}")));
    }
    assert!(pool.len() <= 2 * held.len(), "{} values", pool.len());
    for (index, label) in held.iter().enumerate() {
        assert!(Rc::ptr_eq(
            label,
            &shared(&mut pool, index as u64, &format!("Label {index}"))
        ));
    }
}

#[test]
fn another_content_under_the_same_hash_gives_way() {
    let mut pool = ContentPool::new();
    let first = shared(&mut pool, 7, "first");
    let second = shared(&mut pool, 7, "second");
    assert_eq!(&*first, "first");
    assert_eq!(&*second, "second");
    assert!(Rc::ptr_eq(&second, &shared(&mut pool, 7, "second")));
}
