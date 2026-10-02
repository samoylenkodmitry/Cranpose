use cranpose_native::{NativeFrame, TouchPhase};
use cranpose_native_demo::create_demo_component;

fn has_event(frame: &NativeFrame, name: &str, value: &str) -> bool {
    frame
        .events
        .iter()
        .any(|event| event.name == name && event.value == value)
}

#[test]
fn embedded_component_renders_and_handles_rust_touch() {
    let session = create_demo_component(false, "https://example.com".into());
    let frame = pollster::block_on(session.frame(1110, 780, 3.0)).expect("native frame");
    assert!(frame.pixels[..3].iter().all(|value| *value > 235));
    assert!(frame.slots.is_empty());
    let bottom = ((779 * 1110 + 1109) * 4) as usize;
    assert!(
        frame.pixels[bottom..bottom + 3]
            .iter()
            .all(|value| *value > 235)
    );
    session.touch(TouchPhase::Down, 60.0, 96.0).expect("press");
    session.touch(TouchPhase::Up, 60.0, 96.0).expect("release");
    let frame = pollster::block_on(session.frame(1110, 780, 3.0)).expect("touch frame");
    assert!(has_event(&frame, "count", "1"));
    for (width, height, density) in [(720, 1200, 2.0), (1110, 780, 3.0)] {
        let frame = pollster::block_on(session.frame(width, height, density)).expect("resize");
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
fn webview_keeps_its_url_and_identity_and_reports_load_results() {
    let session = create_demo_component(true, "https://example.com".into());
    let first = pollster::block_on(session.frame(800, 1100, 2.0)).expect("native frame");
    assert_eq!(first.pixels.len(), 800 * 1100 * 4);
    assert_eq!(first.slots.len(), 1);
    assert!(has_event(&first, "count", "0"));
    assert_eq!(first.slots[0].value, "https://example.com");
    let id = first.slots[0].id;
    session
        .send_event("increment".into(), String::new())
        .expect("native button");
    let next = pollster::block_on(session.frame(800, 1100, 2.0)).expect("native update");
    assert!(has_event(&next, "count", "1"));
    assert_eq!(next.slots[0].id, id);
    assert_eq!(next.slots[0].value, first.slots[0].value);
    session
        .native_event(id, "loaded:https://example.com".into())
        .expect("loaded");
    let next = pollster::block_on(session.frame(800, 1100, 2.0)).expect("page loaded");
    assert!(has_event(
        &next,
        "web-status",
        "Loaded: https://example.com"
    ));
    session
        .native_event(id, "error:Offline".into())
        .expect("failed");
    let next = pollster::block_on(session.frame(800, 1100, 2.0)).expect("page failed");
    assert!(has_event(&next, "web-status", "Failed: Offline"));
    session.shutdown();
    assert!(pollster::block_on(session.frame(800, 1100, 2.0)).is_err());
}
