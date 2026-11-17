// What a running output device reports about its timing, through the public backend.

#![cfg(all(
    feature = "cpal-backend",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cranpose_audio::{
    RenderStatus,
    backend::{self, Renderer},
};

struct Silence {
    rendered: Arc<AtomicBool>,
}

impl Renderer for Silence {
    fn set_device_format(&mut self, _sample_rate: f32, _channels: usize) {}

    fn render(&mut self, out: &mut [f32]) -> RenderStatus {
        out.fill(0.0);
        self.rendered.store(true, Ordering::Release);
        RenderStatus::Continue
    }
}

// Polls `done` until it is true, for two seconds at most, and tells whether it became true. The
// device's thread runs the renderer, and what the sink reports is the only signal that it did.
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for the device's own thread, and a poll is the way to see its values"
)]
fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !done() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
    true
}

// A runner without a working output device cannot run this, so the test is for a machine that
// has one: `cargo test -p cranpose-audio --features cpal-backend --test output_latency -- --ignored`
#[test]
#[ignore = "needs a working audio output device"]
fn a_running_device_reports_its_latency_and_burst() {
    let rendered = Arc::new(AtomicBool::new(false));
    let sink = backend::open(Box::new(Silence {
        rendered: Arc::clone(&rendered),
    }))
    .expect("an output device");
    assert!(wait_until(|| rendered.load(Ordering::Acquire)));
    assert!(wait_until(|| sink.output_latency().is_some()));

    let latency = sink.output_latency().expect("a latency after a callback");
    assert!(latency > Duration::ZERO && latency < Duration::from_secs(1));
    assert!(sink.frames_per_burst().is_some_and(|frames| frames > 0));
    // cpal has no count of the device's underruns
    assert_eq!(sink.underrun_count(), None);
}
