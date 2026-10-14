use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

mod support;

use cranpose_coroflow::viewModel;
use cranpose_navigation::{NavController, NavHost, rememberNavController};
use cranpose_services::push_back_request;
use support::{DropMarker, Host, Screen};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Name,
    Address,
}

/// What the screens of a sign-up flow share, as the view model of a nested
/// `navigation()` graph is shared on Android.
struct FlowModel {
    _dropped: DropMarker,
}

type Shared<T> = Rc<Cell<Option<T>>>;

// One test per binary: back requests are process-wide.
#[test]
fn a_host_nested_in_a_screen_is_a_nested_graph() {
    let (outer, inner) = (Shared::default(), Shared::default());
    let (flows, dropped) = (Rc::new(Cell::new(0)), Arc::new(AtomicUsize::new(0)));
    let (outer_sink, inner_sink) = (Rc::clone(&outer), Rc::clone(&inner));
    let (flow_sink, drop_sink) = (Rc::clone(&flows), Arc::clone(&dropped));
    let mut host = Host::new(move || {
        let nav = rememberNavController(Screen::Home);
        outer_sink.set(Some(nav));
        let (inner_sink, flow_sink, drop_sink) = (
            Rc::clone(&inner_sink),
            Rc::clone(&flow_sink),
            Arc::clone(&drop_sink),
        );
        NavHost(nav, move |screen| {
            if screen != Screen::Settings {
                return;
            }
            let (flow_sink, drop_sink) = (Rc::clone(&flow_sink), Arc::clone(&drop_sink));
            let flow = viewModel((), move |_| {
                flow_sink.set(flow_sink.get() + 1);
                FlowModel {
                    _dropped: DropMarker(drop_sink),
                }
            });
            let steps = rememberNavController(Step::Name);
            inner_sink.set(Some(steps));
            NavHost(steps, move |_step| {
                let _shared = flow.get();
            });
        });
    });
    let outer: NavController<Screen> = outer.get().expect("the outer host composed");
    outer.navigate(Screen::Settings);
    host.settle();
    let steps: NavController<Step> = inner.get().expect("the flow composed its host");
    steps.navigate(Step::Address);
    host.settle();
    assert_eq!(flows.get(), 1, "the flow's screens share one model");

    push_back_request();
    host.settle();
    assert_eq!(
        steps.back_stack(),
        [Step::Name],
        "back pops the inner graph first"
    );
    assert_eq!(outer.back_stack(), [Screen::Home, Screen::Settings]);

    push_back_request();
    host.settle();
    assert_eq!(
        outer.back_stack(),
        [Screen::Home],
        "at the inner start, back leaves the graph"
    );
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "the flow's model went with the graph"
    );
}
