use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_services::{clear_opened_links, publish_opened_link, rememberOpenedLinks};

use crate::composition_support::{pump_until, render_collector, serial};

fn collect_links(composition: &mut Composition<MemoryApplier>) -> Rc<RefCell<Vec<String>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let key = location_key(file!(), line!(), column!());
    render_collector(composition, key, rememberOpenedLinks, String::clone, &seen);
    seen
}

#[test]
fn the_link_that_opened_the_app_reaches_the_first_collector() {
    let _guard = serial();
    clear_opened_links();

    publish_opened_link("demo://detail/7");

    let mut composition = Composition::new(MemoryApplier::new());
    let seen = collect_links(&mut composition);
    assert!(
        pump_until(&mut composition, || !seen.borrow().is_empty()),
        "the launch link never reached the collector"
    );
    assert_eq!(*seen.borrow(), ["demo://detail/7"]);
}

#[test]
fn a_link_from_another_thread_reaches_a_live_collector() {
    let _guard = serial();
    clear_opened_links();

    let mut composition = Composition::new(MemoryApplier::new());
    let seen = collect_links(&mut composition);
    assert!(pump_until(&mut composition, || true));

    std::thread::spawn(|| publish_opened_link("demo://settings"))
        .join()
        .expect("the publisher finishes");
    assert!(
        pump_until(&mut composition, || !seen.borrow().is_empty()),
        "the link never reached the live collector"
    );
    assert_eq!(*seen.borrow(), ["demo://settings"]);
}
