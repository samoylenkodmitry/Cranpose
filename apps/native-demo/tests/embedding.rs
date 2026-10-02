use cranpose_native_demo::{DemoSession, FrameListener};

struct Listener;
impl FrameListener for Listener {
    fn request_frame(&self) {}
}

#[test]
fn embedded_component_renders_and_handles_rust_touch() {
    let session = DemoSession::new(false, Box::new(Listener));
    let frame = pollster::block_on(session.frame(1110, 780, 3.0)).expect("native frame");
    assert!(frame.pixels[..3].iter().all(|value| *value > 235));
    assert!(frame.slots.is_empty());
    let bottom = ((779 * 1110 + 1109) * 4) as usize;
    assert!(
        frame.pixels[bottom..bottom + 3]
            .iter()
            .all(|value| *value > 235),
        "background must fill the resized component"
    );
    session
        .touch(cranpose_native_demo::TouchPhase::Down, 60.0, 96.0)
        .expect("press");
    session
        .touch(cranpose_native_demo::TouchPhase::Up, 60.0, 96.0)
        .expect("release");
    let frame = pollster::block_on(session.frame(1110, 780, 3.0)).expect("touch frame");
    assert_eq!(frame.count, 1);
    assert!(frame.pixels[..3].iter().all(|value| *value > 235));
    for (width, height, density) in [(720, 1200, 2.0), (1110, 780, 3.0)] {
        let frame = pollster::block_on(session.frame(width, height, density)).expect("resize");
        assert_eq!(frame.count, 1);
        let corner = frame.pixels.len() - 4;
        assert!(
            frame.pixels[corner..corner + 3]
                .iter()
                .all(|value| *value > 235)
        );
    }
    session.shutdown();
}

#[test]
fn native_host_and_webview_share_state_and_close_cleanly() {
    let session = DemoSession::new(true, Box::new(Listener));
    let frame = pollster::block_on(session.frame(640, 960, 2.0)).expect("native frame");
    assert_eq!(frame.pixels.len(), 640 * 960 * 4);
    assert!(
        frame.pixels[..3].iter().all(|value| *value > 235),
        "root background: {:?}",
        &frame.pixels[..4]
    );
    assert_eq!(frame.slots.len(), 1);
    assert_eq!(frame.count, 0);
    for _ in 0..12 {
        let idle = pollster::block_on(session.frame(640, 960, 2.0)).expect("idle frame");
        if !idle.pixels.is_empty() {
            assert!(
                idle.pixels[..3].iter().all(|value| *value > 235),
                "idle background: {:?}",
                &idle.pixels[..4]
            );
        }
    }
    session.increment().expect("native button");
    let next = pollster::block_on(session.frame(640, 960, 2.0)).expect("native update");
    assert_eq!(next.count, 1);
    assert!(
        next.pixels[..3].iter().all(|value| *value > 235),
        "updated background: {:?}",
        &next.pixels[..4]
    );
    session
        .native_event(frame.slots[0].id, "increment".into())
        .expect("WebView event");
    let next = pollster::block_on(session.frame(640, 960, 2.0)).expect("WebView update");
    assert_eq!(next.count, 2);
    assert!(next.slots[0].value.contains("Count: 2"));
    session.shutdown();
    assert!(pollster::block_on(session.frame(640, 960, 2.0)).is_err());
}
