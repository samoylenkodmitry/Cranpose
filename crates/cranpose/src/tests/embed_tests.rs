use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use cranpose_app_shell::FrameSchedule;
use cranpose_core::{collectAsState, rememberMutableStateOf};
use cranpose_services::rememberHostMessages;
use cranpose_ui::{Box, BoxSpec, Color, Modifier, composable};

use super::*;
use crate::embed_protocol::PixelRect;

const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);
const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);
const WHITE: Color = Color(1.0, 1.0, 1.0, 1.0);

const BGRA_RED: [u8; 4] = [0, 0, 255, 255];
const BGRA_GREEN: [u8; 4] = [0, 255, 0, 255];
const BGRA_BLUE: [u8; 4] = [255, 0, 0, 255];
const BGRA_WHITE: [u8; 4] = [255, 255, 255, 255];

fn gpu_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[composable]
fn color_probe(channel: &'static str) {
    let background = collectAsState(rememberHostMessages(channel), (), String::new());
    let pressed = rememberMutableStateOf(|| false);
    let fill = if background.get() == "green" {
        GREEN
    } else {
        RED
    };
    Box(
        Modifier::empty().fill_max_size().background(fill),
        BoxSpec::new(),
        move || {
            let square = if pressed.get() { BLUE } else { WHITE };
            Box(
                Modifier::empty()
                    .offset(10.0, 10.0)
                    .size_points(20.0, 20.0)
                    .background(square)
                    .clickable(move |_| pressed.set(!pressed.get())),
                BoxSpec::new(),
                || {},
            );
        },
    );
}

fn probe_host(channel: &'static str, width: u32, height: u32) -> EmbeddedHost {
    let gpu = HeadlessGpu::request().expect("an adapter that draws off screen");
    EmbeddedHost::new(
        &AppSettings::default(),
        gpu,
        PlatformEnvironment::new(),
        move || color_probe(channel),
        SurfaceSize::clamped(width, height, 1.0, 60.0),
    )
}

fn pixel(frame: &FrameUpdate<'_>, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * frame.buffer_width + x) * 4) as usize;
    let mut bgra = [0u8; 4];
    bgra.copy_from_slice(&frame.pixels[offset..offset + 4]);
    bgra
}

fn next_frame_where(
    host: &mut EmbeddedHost,
    accept: impl Fn(&FrameUpdate<'_>) -> bool,
) -> Option<(PixelRect, [u8; 4])> {
    for _ in 0..10 {
        if let Some(frame) = host.produce_frame().expect("the frame renders")
            && accept(&frame)
        {
            return Some((frame.rect, pixel(&frame, 50, 40)));
        }
    }
    None
}

#[test]
fn an_endpoint_needs_both_variables() {
    let lookup = |name: &str| match name {
        EmbedEndpoint::ADDRESS_VARIABLE => Some("127.0.0.1:4000".to_string()),
        EmbedEndpoint::TOKEN_VARIABLE => Some("secret".to_string()),
        _ => None,
    };
    let endpoint = EmbedEndpoint::from_lookup(lookup).expect("both variables are set");
    assert_eq!(endpoint, EmbedEndpoint::new("127.0.0.1:4000", "secret"));
    assert_eq!(endpoint.address(), "127.0.0.1:4000");

    let address_only = |name: &str| {
        (name == EmbedEndpoint::ADDRESS_VARIABLE).then(|| "127.0.0.1:4000".to_string())
    };
    assert_eq!(EmbedEndpoint::from_lookup(address_only), None);
}

#[test]
fn a_process_started_without_a_host_has_no_endpoint() {
    assert_eq!(EmbedEndpoint::from_env(), None);
}

#[test]
fn surface_sizes_are_clamped_to_drawable_values() {
    let size = SurfaceSize::clamped(0, 0, f32::NAN, 0.0);
    assert_eq!(
        size,
        SurfaceSize {
            width: 1,
            height: 1,
            scale: 1.0,
            refresh_hz: DEFAULT_REFRESH_HZ,
        }
    );

    let retina = SurfaceSize::clamped(800, 600, 2.0, 120.0);
    assert_eq!(retina.viewport(), (400.0, 300.0));
    assert_eq!(
        retina.frame_interval(),
        Duration::from_secs_f64(1.0 / 120.0)
    );
}

#[test]
fn the_loop_sleeps_until_the_next_frame_or_deadline() {
    let now = Instant::now();
    let interval = Duration::from_millis(16);
    let idle = FrameSchedule {
        needs_update: false,
        needs_frame: false,
        next_deadline: None,
    };
    let mut pacing = Pacing::new();
    assert_eq!(pacing.next_wake(idle, true, interval, now), Some(now));

    pacing.ticked(now);
    assert_eq!(pacing.next_wake(idle, false, interval, now), None);
    assert_eq!(
        pacing.next_wake(idle, true, interval, now),
        Some(now + interval),
        "a dirty surface wakes one interval after the last tick"
    );

    let blinking = FrameSchedule {
        next_deadline: Some(now + Duration::from_millis(500)),
        ..idle
    };
    assert_eq!(
        pacing.next_wake(blinking, false, interval, now),
        Some(now + Duration::from_millis(500))
    );

    let animating = FrameSchedule {
        needs_frame: true,
        ..idle
    };
    for _ in 0..MAX_FRAMES_IN_FLIGHT {
        pacing.sent_frame();
    }
    assert_eq!(
        pacing.next_wake(animating, false, interval, now),
        None,
        "with no room at the host only its acknowledgement wakes the loop"
    );
    pacing.acknowledged();
    assert_eq!(
        pacing.next_wake(animating, false, interval, now),
        Some(now + interval)
    );
}

#[test]
fn ticks_wait_for_the_interval_and_for_room_at_the_host() {
    let now = Instant::now();
    let interval = Duration::from_millis(16);
    let mut pacing = Pacing::new();
    assert!(pacing.may_tick(interval, now));

    pacing.ticked(now);
    assert!(!pacing.may_tick(interval, now + Duration::from_millis(5)));
    assert!(pacing.may_tick(interval, now + interval));

    for _ in 0..MAX_FRAMES_IN_FLIGHT {
        pacing.sent_frame();
    }
    assert!(!pacing.may_tick(interval, now + interval * 4));
}

#[test]
fn system_cursors_keep_their_css_names() {
    assert_eq!(cursor_name(&PointerIcon::DEFAULT), "default");
    assert_eq!(
        cursor_name(&PointerIcon::System(cranpose_ui::CursorIcon::Text)),
        "text"
    );
}

#[test]
fn frames_carry_only_what_changed() {
    let _gpu = gpu_lock();
    let mut host = probe_host("embed-test.frames", 64, 48);

    let frame = host
        .produce_frame()
        .expect("the first frame renders")
        .expect("the first frame is sent");
    assert_eq!(frame.rect, PixelRect::full(64, 48));
    assert_eq!((frame.buffer_width, frame.buffer_height), (64, 48));
    assert_eq!(pixel(&frame, 50, 40), BGRA_RED);
    assert_eq!(pixel(&frame, 15, 15), BGRA_WHITE);

    assert!(
        host.produce_frame().expect("renders").is_none(),
        "an unchanged surface sends nothing"
    );

    host.handle(HostEvent::PointerMove { x: 15.0, y: 15.0 });
    host.handle(HostEvent::PointerDown { x: 15.0, y: 15.0 });
    host.handle(HostEvent::PointerUp { x: 15.0, y: 15.0 });
    let mut pressed = None;
    for _ in 0..10 {
        if let Some(frame) = host.produce_frame().expect("renders") {
            pressed = Some((frame.rect, pixel(&frame, 15, 15)));
            break;
        }
    }
    let (rect, square) = pressed.expect("the clicked square repaints");
    assert_eq!(square, BGRA_BLUE);
    assert!(
        rect.x >= 10 && rect.y >= 10 && rect.x + rect.width <= 30 && rect.y + rect.height <= 30,
        "only the square changed, got {rect:?}"
    );
}

#[test]
fn a_host_message_reaches_the_composition() {
    let _gpu = gpu_lock();
    let mut host = probe_host("embed-test.message", 64, 48);
    assert!(host.produce_frame().expect("renders").is_some());

    host.handle(HostEvent::Message {
        channel: "embed-test.message".to_string(),
        payload: "green".to_string(),
    });

    let recolored = next_frame_where(&mut host, |frame| pixel(frame, 50, 40) == BGRA_GREEN);
    assert!(recolored.is_some(), "the background never turned green");
}

#[test]
fn a_resize_sends_a_whole_frame_of_the_new_size() {
    let _gpu = gpu_lock();
    let mut host = probe_host("embed-test.resize", 64, 48);
    assert!(host.produce_frame().expect("renders").is_some());

    host.handle(HostEvent::Resize {
        width: 80,
        height: 60,
        scale: 2.0,
        refresh_hz: 60.0,
    });
    assert_eq!(host.size(), SurfaceSize::clamped(80, 60, 2.0, 60.0));

    let frame = host
        .produce_frame()
        .expect("renders")
        .expect("a resized surface is redrawn");
    assert_eq!(frame.rect, PixelRect::full(80, 60));
    assert_eq!(
        pixel(&frame, 25, 25),
        BGRA_WHITE,
        "the square doubled with the scale"
    );
}

struct ClosingWriter {
    bytes: Vec<u8>,
    close: Sender<LoopEvent>,
}

impl Write for ClosingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.bytes.is_empty() {
            let _ = self.close.send(LoopEvent::Host(HostEvent::Close));
        }
        Ok(())
    }
}

fn message_kinds(mut bytes: &[u8]) -> Vec<u8> {
    let mut kinds = Vec::new();
    while let Some((length, rest)) = bytes.split_first_chunk::<4>() {
        let length = u32::from_le_bytes(*length) as usize;
        kinds.push(rest[0]);
        bytes = &rest[length..];
    }
    kinds
}

#[test]
fn serving_sends_queued_messages_then_the_first_frame() {
    let _gpu = gpu_lock();
    let mut host = probe_host("embed-test.serve", 32, 32);
    let (sender, events) = mpsc::channel();
    sender
        .send(LoopEvent::Outgoing(HostMessage::new("ide.notify", "hello")))
        .expect("the loop's channel is open");
    let mut writer = ClosingWriter {
        bytes: Vec::new(),
        close: sender,
    };

    serve(&mut host, &events, &mut writer).expect("the loop ends when the host closes");

    assert_eq!(message_kinds(&writer.bytes), vec![0x04, 0x02]);
}

#[test]
fn a_hidden_surface_draws_nothing_until_shown() {
    let _gpu = gpu_lock();
    let mut host = probe_host("embed-test.visibility", 16, 16);
    host.handle(HostEvent::Visibility(false));
    assert!(!host.visible());

    let (sender, events) = mpsc::channel();
    sender
        .send(LoopEvent::Host(HostEvent::Visibility(true)))
        .expect("the loop's channel is open");
    let mut writer = ClosingWriter {
        bytes: Vec::new(),
        close: sender,
    };
    serve(&mut host, &events, &mut writer).expect("the loop ends when the host closes");

    assert!(host.visible());
    assert_eq!(message_kinds(&writer.bytes), vec![0x02]);
}

#[test]
fn a_disconnected_host_ends_the_loop() {
    let _gpu = gpu_lock();
    let mut host = probe_host("embed-test.disconnect", 16, 16);
    let (sender, events) = mpsc::channel();
    sender
        .send(LoopEvent::Disconnected)
        .expect("the loop's channel is open");

    let mut sink = Vec::new();
    serve(&mut host, &events, &mut sink).expect("a disconnect ends the loop cleanly");
    assert!(sink.is_empty());
}
