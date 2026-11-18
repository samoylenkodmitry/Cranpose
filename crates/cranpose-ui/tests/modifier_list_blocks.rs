// A modifier of several elements keeps them in a block that its copies
// share. The block goes back to the thread when its last holder drops, and
// the thread's next list takes it. A copy keeps its elements while other
// holders drop, a list made in a block given back holds only its own
// elements, and modifiers that drop while their thread exits drop cleanly.

use std::cell::RefCell;

use cranpose_ui::{Color, Modifier};

fn chain(padding: f32) -> Modifier {
    Modifier::empty()
        .padding(padding)
        .background(Color::RED)
        .rounded_corners(3.0)
}

#[test]
fn a_copy_keeps_its_elements_when_another_holder_drops() {
    let first = chain(1.0);
    let copy = first.clone();
    drop(first);
    let extended = copy.clone().then(Modifier::empty().padding(2.0));
    drop(chain(5.0));

    assert_eq!(copy, chain(1.0));
    assert_eq!(copy.total_padding(), 1.0);
    assert_eq!(extended, chain(1.0).then(Modifier::empty().padding(2.0)));
    assert_eq!(extended.total_padding(), 3.0);
}

#[test]
fn a_list_in_a_block_given_back_holds_only_its_own_elements() {
    // Five elements: more than a block holds in place.
    drop(
        Modifier::empty()
            .padding(10.0)
            .padding(20.0)
            .padding(30.0)
            .padding(40.0)
            .padding(50.0),
    );
    let reused = Modifier::empty().padding(1.0).padding(2.0);

    assert_eq!(reused.total_padding(), 3.0);
    assert_eq!(
        reused,
        Modifier::empty()
            .padding(1.0)
            .then(Modifier::empty().padding(2.0))
    );
}

thread_local! {
    static KEPT_BEFORE_THE_UI_STATE: RefCell<Option<Modifier>> = const { RefCell::new(None) };
    static KEPT_AFTER_THE_UI_STATE: RefCell<Option<Modifier>> = const { RefCell::new(None) };
}

#[test]
fn modifiers_that_drop_as_their_thread_exits_drop_cleanly() {
    std::thread::spawn(|| {
        // This thread local starts before the thread's ui state, which the
        // chain's join starts.
        KEPT_BEFORE_THE_UI_STATE.with(|kept| *kept.borrow_mut() = Some(chain(1.0)));
        drop(chain(2.0));
        let last = chain(3.0);
        KEPT_AFTER_THE_UI_STATE.with(|kept| *kept.borrow_mut() = Some(last));
    })
    .join()
    .expect("the thread exits without a panic");
}
