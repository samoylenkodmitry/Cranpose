mod support;

use cranpose_navigation::{BindDeepLinks, NavHost, rememberNavController};
use cranpose_services::{clear_opened_links, publish_opened_link};
use support::{Host, Screen, SharedNav};

fn route_of(link: &str) -> Option<Screen> {
    let path = link.strip_prefix("demo://")?;
    match path {
        "home" => Some(Screen::Home),
        "settings" => Some(Screen::Settings),
        _ => path
            .strip_prefix("detail/")?
            .parse()
            .ok()
            .map(Screen::Detail),
    }
}

// One test per binary: opened links are process-wide.
#[test]
fn a_link_shows_its_route_over_the_start() {
    clear_opened_links();
    publish_opened_link("demo://detail/7");
    let nav = SharedNav::default();
    let captured = std::rc::Rc::clone(&nav);
    let mut host = Host::new(move || {
        let controller = rememberNavController(Screen::Home);
        captured.set(Some(controller));
        BindDeepLinks(controller, route_of);
        NavHost(controller, |_| {});
    });
    host.settle();
    let nav = nav.get().expect("the host composed its controller");
    assert_eq!(
        nav.back_stack(),
        [Screen::Home, Screen::Detail(7)],
        "the link that launched the app waited for the binding"
    );

    nav.navigate(Screen::Detail(8));
    publish_opened_link("demo://settings");
    host.settle();
    assert_eq!(
        nav.back_stack(),
        [Screen::Home, Screen::Settings],
        "a link while the app runs starts over from the start entry"
    );

    publish_opened_link("https://example.com/unknown");
    host.settle();
    assert_eq!(nav.back_stack(), [Screen::Home, Screen::Settings]);

    publish_opened_link("demo://home");
    host.settle();
    assert_eq!(nav.back_stack(), [Screen::Home], "a link to the start");
}
