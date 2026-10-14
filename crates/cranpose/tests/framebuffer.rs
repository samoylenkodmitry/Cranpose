use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose::{framebuffer::Framebuffer, prelude::*};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};

const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);

fn center(framebuffer: &mut Framebuffer) -> [u8; 4] {
    let (width, height) = (framebuffer.width() as usize, framebuffer.height() as usize);
    let offset = 4 * (height / 2 * width + width / 2);
    let mut pixel = [0; 4];
    pixel.copy_from_slice(&framebuffer.pixels()[offset..offset + 4]);
    pixel
}

#[test]
fn the_host_theme_reaches_content() {
    for (theme, dark) in [(SystemTheme::Dark, true), (SystemTheme::Light, false)] {
        let seen = Rc::new(Cell::new(None));
        let sink = Rc::clone(&seen);
        let mut framebuffer = Framebuffer::new(40, 30, 1.0, theme, move || {
            sink.set(Some(isSystemInDarkTheme()));
        })
        .expect("valid surface");
        framebuffer.set_active(true);
        framebuffer.tick(1);
        assert_eq!(seen.get(), Some(dark));
    }
}

#[test]
fn a_still_picture_lets_the_host_sleep() {
    let mut framebuffer = Framebuffer::new(40, 30, 1.0, SystemTheme::Light, || {
        Spacer(Modifier::empty().fill_max_size().background(RED));
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    assert!(framebuffer.tick(1));
    assert_eq!(center(&mut framebuffer), [255, 0, 0, 255]);
    let schedule = framebuffer.frame_schedule();
    assert!(!schedule.needs_update);
    assert!(!schedule.needs_frame);
    assert_eq!(schedule.next_deadline, None);
    assert!(!framebuffer.tick(16_000_001));
}

#[test]
fn an_animation_asks_for_the_next_frame() {
    let mut framebuffer = Framebuffer::new(40, 30, 1.0, SystemTheme::Light, || {
        let transition = rememberInfiniteTransition("sweep");
        let spec = infiniteRepeatable(
            AnimationSpec::linear(100),
            RepeatMode::Restart,
            StartOffset::default(),
        );
        let fraction = transition.animateFloat(0.0, 1.0, spec, "fraction").value();
        Spacer(
            Modifier::empty()
                .fill_max_width_fraction(fraction)
                .fill_max_height()
                .background(RED),
        );
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    framebuffer.tick(1);
    let schedule = framebuffer.frame_schedule();
    assert!(schedule.needs_frame || schedule.needs_update);
    assert!(framebuffer.tick(60_000_000));
}

#[test]
fn a_delay_wakes_a_sleeping_host_and_its_change_presents() {
    let mut framebuffer = Framebuffer::new(40, 30, 1.0, SystemTheme::Light, || {
        let done = cranpose_core::rememberMutableStateOf(|| false);
        cranpose_core::LaunchedEffectAsync((), move |_| {
            std::boxed::Box::pin(async move {
                cranpose_core::delay(Duration::from_millis(20)).await;
                done.set(true);
            })
        });
        let color = if done.get() { GREEN } else { RED };
        Spacer(Modifier::empty().fill_max_size().background(color));
    })
    .expect("valid surface");
    let woken = Arc::new(AtomicBool::new(false));
    let sink = Arc::clone(&woken);
    framebuffer.set_frame_waker(move || sink.store(true, Ordering::Release));
    framebuffer.set_active(true);
    framebuffer.tick(1);
    assert_eq!(center(&mut framebuffer), [255, 0, 0, 255]);
    let schedule = framebuffer.frame_schedule();
    assert!(!schedule.needs_frame);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut frame_time = 1;
    while center(&mut framebuffer) != [0, 255, 0, 255] {
        assert!(Instant::now() < deadline, "the delay never woke the host");
        if woken.swap(false, Ordering::AcqRel) {
            framebuffer.run_pending_tasks();
            frame_time += 16_000_000;
            framebuffer.tick(frame_time);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
