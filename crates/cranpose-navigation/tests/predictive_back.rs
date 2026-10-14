mod support;

use cranpose_services::{BackEdge, BackEvent, BackGesture, push_back_gesture, push_back_request};
use support::{Probe, Screen, screens};

fn moved(progress: f32) -> BackEvent {
    BackEvent {
        progress,
        edge: BackEdge::Left,
    }
}

fn sorted(mut screens: Vec<Screen>) -> Vec<Screen> {
    screens.sort_by_key(|screen| format!("{screen:?}"));
    screens
}

// One test per binary: back requests and gestures are process-wide.
#[test]
fn a_back_gesture_shows_the_screen_beneath_then_pops_or_goes_back() {
    let probe = Probe::default();
    let (mut host, nav) = screens(&probe);
    let nav = nav.get().expect("the host composed its controller");
    nav.navigate(Screen::Detail(1));
    host.settle();
    let built = probe.built().len();

    push_back_gesture(BackGesture::Started(moved(0.0)));
    push_back_gesture(BackGesture::Progressed(moved(0.4)));
    host.run_frames(1);
    assert_eq!(
        sorted(probe.live()),
        [Screen::Detail(1), Screen::Home],
        "the screen beneath shows during the gesture"
    );
    assert_eq!(nav.back_stack(), [Screen::Home, Screen::Detail(1)]);

    push_back_gesture(BackGesture::Cancelled);
    host.settle();
    assert_eq!(
        probe.live(),
        [Screen::Detail(1)],
        "a cancelled gesture goes back"
    );
    assert_eq!(nav.back_stack(), [Screen::Home, Screen::Detail(1)]);

    push_back_gesture(BackGesture::Started(moved(0.0)));
    push_back_gesture(BackGesture::Progressed(moved(0.7)));
    host.run_frames(1);
    push_back_request();
    host.settle();
    assert_eq!(nav.back_stack(), [Screen::Home]);
    assert_eq!(probe.live(), [Screen::Home]);
    assert_eq!(
        probe.built().len(),
        built,
        "the screen beneath keeps its view models through the gesture"
    );

    push_back_gesture(BackGesture::Started(moved(0.0)));
    push_back_gesture(BackGesture::Progressed(moved(0.5)));
    host.run_frames(1);
    assert_eq!(
        probe.live(),
        [Screen::Home],
        "with nothing beneath, a gesture shows nothing"
    );
}
