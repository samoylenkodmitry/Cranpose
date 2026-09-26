use std::{
    cell::Cell,
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
};

use cranpose_app_shell::FrameSchedule;
use cranpose_core::{collectAsState, rememberMutableStateOf};
use cranpose_services::rememberHostMessages;
use cranpose_ui::{Box, BoxSpec, Color, Modifier, Point, composable};

use super::*;

#[test]
fn inspection_request_returns_the_primary_surface_report() {
    let _lock = gpu_lock();
    let mut host = new_host(320, 240, || color_probe("inspector-test"));
    host.update();
    let report = host.shell.debug_info_report();
    assert!(!report.is_empty());
    let mut expected = Vec::new();
    write_app_event(
        &mut expected,
        &AppEvent::Message {
            channel: INSPECT_SNAPSHOT_CHANNEL,
            payload: &report,
        },
    )
    .expect("encode report");
    let mut output = Vec::new();
    assert!(
        apply_batch(
            &mut host,
            &mut output,
            vec![LoopEvent::Host(HostEvent::Message {
                channel: INSPECT_REQUEST_CHANNEL.into(),
                payload: String::new(),
            })]
        )
        .expect("inspect")
    );
    assert_eq!(output, expected);
    output.clear();
    assert!(apply_batch(&mut host, &mut output, vec![LoopEvent::Wake]).expect("idle wake"));
    assert!(
        output.is_empty(),
        "reports must be requested, not emitted on every wake"
    );
}
use crate::{
    WindowConfig, WindowModifierExt,
    embed_protocol::{PixelRect, WINDOW_DECORATED, WINDOW_TRANSPARENT, WindowSpec},
    embed_surfaces::MAX_FRAMES_IN_FLIGHT,
    native_window::{WindowState, rememberWindowStateAt},
};

const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);
const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);
const WHITE: Color = Color(1.0, 1.0, 1.0, 1.0);
const HALF_RED: Color = Color(1.0, 0.0, 0.0, 0.5);

const BGRA_RED: [u8; 4] = [0, 0, 255, 255];
const BGRA_GREEN: [u8; 4] = [0, 255, 0, 255];
const BGRA_BLUE: [u8; 4] = [255, 0, 0, 255];
const BGRA_WHITE: [u8; 4] = [255, 255, 255, 255];
const BGRA_CLEAR: [u8; 4] = [0, 0, 0, 0];

thread_local! {
    static WINDOW_STATE: Cell<Option<WindowState>> = const { Cell::new(None) };
    static CLOSE_REQUESTS: Cell<u32> = const { Cell::new(0) };
}

fn gpu_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

struct Captured {
    surface: SurfaceId,
    rect: PixelRect,
    buffer: (u32, u32),
    pixels: Vec<u8>,
}

impl Captured {
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * self.buffer.0 + x) * 4) as usize;
        let mut bgra = [0u8; 4];
        bgra.copy_from_slice(&self.pixels[offset..offset + 4]);
        bgra
    }
}

fn surface(surface: SurfaceId, event: SurfaceEvent) -> HostEvent {
    HostEvent::Surface { surface, event }
}

fn resize(width: u32, height: u32, scale: f32) -> SurfaceEvent {
    SurfaceEvent::Resize {
        width,
        height,
        scale,
        refresh_hz: 60.0,
    }
}

fn tick(host: &mut EmbeddedHost) -> (Vec<SurfaceCommand>, Vec<Captured>) {
    host.update();
    let commands = host.take_commands();
    let mut frames = Vec::new();
    host.produce_frames(|frame| {
        frames.push(Captured {
            surface: frame.surface,
            rect: frame.rect,
            buffer: (frame.buffer_width, frame.buffer_height),
            pixels: frame.pixels.to_vec(),
        });
        Ok(())
    })
    .expect("frames render");
    for frame in &frames {
        host.handle(surface(frame.surface, SurfaceEvent::FrameAck(0)));
    }
    (commands, frames)
}

fn frame_of(frames: Vec<Captured>, id: SurfaceId) -> Option<Captured> {
    frames.into_iter().find(|frame| frame.surface == id)
}

fn next_frame_of(
    host: &mut EmbeddedHost,
    id: SurfaceId,
    accept: impl Fn(&Captured) -> bool,
) -> Option<Captured> {
    (0..10).find_map(|_| frame_of(tick(host).1, id).filter(|frame| accept(frame)))
}

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
}

fn new_host(width: u32, height: u32, content: impl FnMut() + 'static) -> EmbeddedHost {
    let gpu = HeadlessGpu::request().expect("an adapter that draws off screen");
    EmbeddedHost::new(
        &AppSettings::default(),
        gpu,
        PlatformEnvironment::new(),
        content,
        SurfaceSize::clamped(width, height, 1.0, 60.0),
    )
}

fn opened_window(commands: &[SurfaceCommand]) -> WindowSpec {
    match commands.first() {
        Some(SurfaceCommand::OpenWindow(spec)) => spec.clone(),
        other => panic!("the window is announced before it is drawn, got {other:?}"),
    }
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

#[composable]
fn window_probe(channel: &'static str) {
    let command = collectAsState(rememberHostMessages(channel), (), String::new());
    let state = rememberWindowStateAt(100.0, 80.0, 40.0, 30.0);
    WINDOW_STATE.with(|slot| slot.set(Some(state)));
    Box(
        Modifier::empty().fill_max_size().background(RED),
        BoxSpec::new(),
        || {},
    );
    if command.get() != "close" {
        Box(
            Modifier::empty().window(
                WindowConfig::borderless_for_state("probe", state)
                    .with_transparent(true)
                    .on_close_requested(|| CLOSE_REQUESTS.with(|count| count.set(count.get() + 1))),
            ),
            BoxSpec::new(),
            || {
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .background(HALF_RED)
                        .window_drag_area(|| {}, || {}),
                    BoxSpec::new(),
                    || {},
                );
            },
        );
    }
}

#[composable]
fn overlay_probe(channel: &'static str) {
    let command = collectAsState(rememberHostMessages(channel), (), String::new());
    Box(
        Modifier::empty().fill_max_size().background(RED),
        BoxSpec::new(),
        || {},
    );
    if command.get() != "remove" {
        HostOverlay("editor", || {
            Box(
                Modifier::empty()
                    .offset(4.0, 4.0)
                    .size_points(8.0, 8.0)
                    .background(GREEN),
                BoxSpec::new(),
                || {},
            );
        });
    }
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
    assert_eq!(
        pacing.next_wake(animating, false, interval, now),
        Some(now + interval)
    );
}

#[test]
fn ticks_wait_for_the_interval() {
    let now = Instant::now();
    let interval = Duration::from_millis(16);
    let mut pacing = Pacing::new();
    assert!(pacing.may_tick(interval, now));

    pacing.ticked(now);
    assert!(!pacing.may_tick(interval, now + Duration::from_millis(5)));
    assert!(pacing.may_tick(interval, now + interval));
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
    let mut host = new_host(64, 48, || color_probe("embed-test.frames"));

    let frame = frame_of(tick(&mut host).1, PRIMARY_SURFACE).expect("the first frame is sent");
    assert_eq!(frame.rect, PixelRect::full(64, 48));
    assert_eq!(frame.buffer, (64, 48));
    assert_eq!(frame.pixel(50, 40), BGRA_RED);
    assert_eq!(frame.pixel(15, 15), BGRA_WHITE);

    assert!(
        tick(&mut host).1.is_empty(),
        "an unchanged surface sends nothing"
    );

    for event in [
        SurfaceEvent::PointerMove { x: 15.0, y: 15.0 },
        SurfaceEvent::PointerDown { x: 15.0, y: 15.0 },
        SurfaceEvent::PointerUp { x: 15.0, y: 15.0 },
    ] {
        host.handle(surface(PRIMARY_SURFACE, event));
    }
    let pressed =
        next_frame_of(&mut host, PRIMARY_SURFACE, |_| true).expect("the clicked square repaints");
    assert_eq!(pressed.pixel(15, 15), BGRA_BLUE);
    let rect = pressed.rect;
    assert!(
        rect.x >= 10 && rect.y >= 10 && rect.x + rect.width <= 30 && rect.y + rect.height <= 30,
        "only the square changed, got {rect:?}"
    );
}

#[test]
fn unacknowledged_frames_hold_the_surface_back() {
    let _gpu = gpu_lock();
    let mut host = new_host(32, 32, || color_probe("embed-test.credits"));
    let mut sent = 0;
    for round in 0..3 {
        host.handle(surface(PRIMARY_SURFACE, resize(32 + round, 32, 1.0)));
        host.update();
        host.produce_frames(|_| {
            sent += 1;
            Ok(())
        })
        .expect("frames render");
    }
    assert_eq!(sent, MAX_FRAMES_IN_FLIGHT);
    assert!(!host.can_draw(), "a surface with no room is not drawn");

    host.handle(surface(PRIMARY_SURFACE, SurfaceEvent::FrameAck(1)));
    assert!(host.can_draw());
}

#[test]
fn a_host_message_reaches_the_composition() {
    let _gpu = gpu_lock();
    let mut host = new_host(64, 48, || color_probe("embed-test.message"));
    assert!(frame_of(tick(&mut host).1, PRIMARY_SURFACE).is_some());

    host.handle(HostEvent::Message {
        channel: "embed-test.message".to_string(),
        payload: "green".to_string(),
    });

    let recolored = next_frame_of(&mut host, PRIMARY_SURFACE, |frame| {
        frame.pixel(50, 40) == BGRA_GREEN
    });
    assert!(recolored.is_some(), "the background never turned green");
}

#[test]
fn a_resize_sends_a_whole_frame_of_the_new_size() {
    let _gpu = gpu_lock();
    let mut host = new_host(64, 48, || color_probe("embed-test.resize"));
    assert!(frame_of(tick(&mut host).1, PRIMARY_SURFACE).is_some());

    host.handle(surface(PRIMARY_SURFACE, resize(80, 60, 2.0)));
    assert_eq!(host.size(), SurfaceSize::clamped(80, 60, 2.0, 60.0));

    let frame = frame_of(tick(&mut host).1, PRIMARY_SURFACE).expect("a resized surface is redrawn");
    assert_eq!(frame.rect, PixelRect::full(80, 60));
    assert_eq!(
        frame.pixel(25, 25),
        BGRA_WHITE,
        "the square doubled with the scale"
    );
}

#[test]
fn a_window_opens_as_a_host_surface_drawn_with_alpha() {
    let _gpu = gpu_lock();
    let mut host = new_host(64, 48, || window_probe("embed-test.window-open"));

    let (commands, frames) = tick(&mut host);
    let spec = opened_window(&commands);
    assert_eq!(spec.title, "probe");
    assert_eq!(spec.position, Some((100.0, 80.0)));
    assert_eq!((spec.width, spec.height), (40.0, 30.0));
    assert!(spec.flags & WINDOW_TRANSPARENT != 0 && spec.flags & WINDOW_DECORATED == 0);
    assert!(
        WINDOW_STATE
            .with(Cell::get)
            .is_some_and(WindowState::presented_non_reactive),
        "the window's state knows it is on screen"
    );

    let primary = frames
        .iter()
        .find(|frame| frame.surface == PRIMARY_SURFACE)
        .expect("the primary surface is drawn");
    assert_eq!(primary.pixel(20, 15), BGRA_RED);
    let window = frame_of(frames, spec.surface).expect("the window is drawn");
    assert_eq!(window.buffer, (40, 30));
    assert!(
        close(window.pixel(20, 15), [0, 0, 128, 128]),
        "half-transparent red arrives premultiplied, got {:?}",
        window.pixel(20, 15)
    );
}

#[test]
fn a_window_follows_what_the_host_reports() {
    let _gpu = gpu_lock();
    CLOSE_REQUESTS.with(|count| count.set(0));
    let mut host = new_host(64, 48, || window_probe("embed-test.window-host"));
    let id = opened_window(&tick(&mut host).0).surface;

    host.handle(surface(id, SurfaceEvent::Moved { x: 300.0, y: 200.0 }));
    let state = WINDOW_STATE
        .with(Cell::get)
        .expect("the probe shares its state");
    assert_eq!(
        state.position_non_reactive(),
        Some(Point { x: 300.0, y: 200.0 })
    );
    let (commands, _) = tick(&mut host);
    assert!(
        commands.iter().all(|command| !matches!(
            command,
            SurfaceCommand::OpenWindow(WindowSpec {
                position: Some(_),
                ..
            })
        )),
        "the host is never told to move a window back to where it reported it, got {commands:?}"
    );

    host.handle(surface(id, resize(60, 40, 1.0)));
    let resized = next_frame_of(&mut host, id, |frame| frame.buffer == (60, 40));
    assert!(resized.is_some(), "the window redraws at the host's size");

    host.handle(surface(id, SurfaceEvent::CloseRequested));
    assert_eq!(CLOSE_REQUESTS.with(Cell::get), 1);
}

#[test]
fn a_press_on_a_drag_area_asks_the_host_to_move_the_window() {
    let _gpu = gpu_lock();
    let mut host = new_host(64, 48, || window_probe("embed-test.window-drag"));
    let id = opened_window(&tick(&mut host).0).surface;

    host.handle(surface(id, SurfaceEvent::PointerMove { x: 20.0, y: 15.0 }));
    host.handle(surface(id, SurfaceEvent::PointerDown { x: 20.0, y: 15.0 }));

    assert_eq!(host.take_commands(), vec![SurfaceCommand::BeginMove(id)]);
}

#[test]
fn a_window_that_leaves_the_composition_closes_its_surface() {
    let _gpu = gpu_lock();
    let mut host = new_host(64, 48, || window_probe("embed-test.window-close"));
    let id = opened_window(&tick(&mut host).0).surface;

    host.handle(HostEvent::Message {
        channel: "embed-test.window-close".to_string(),
        payload: "close".to_string(),
    });
    let closed = (0..10).any(|_| tick(&mut host).0.contains(&SurfaceCommand::Close(id)));
    assert!(closed, "the host is told to close the window");
    assert!(
        !WINDOW_STATE
            .with(Cell::get)
            .is_some_and(WindowState::presented_non_reactive)
    );
}

#[test]
fn an_overlay_draws_transparently_once_the_host_sizes_it() {
    let _gpu = gpu_lock();
    let mut host = new_host(64, 48, || overlay_probe("embed-test.overlay"));

    let (commands, frames) = tick(&mut host);
    let Some(SurfaceCommand::OpenOverlay {
        surface: id,
        anchor,
    }) = commands.first().cloned()
    else {
        panic!("the overlay is announced, got {commands:?}");
    };
    assert_eq!(anchor, "editor");
    assert!(
        frames.iter().all(|frame| frame.surface == PRIMARY_SURFACE),
        "nothing is drawn for an overlay the host has not sized"
    );
    let primary = frame_of(frames, PRIMARY_SURFACE).expect("the primary surface is drawn");
    assert_eq!(
        primary.pixel(6, 6),
        BGRA_RED,
        "the overlay's content stays out of the primary surface"
    );

    host.handle(surface(id, resize(32, 24, 1.0)));
    let overlay = next_frame_of(&mut host, id, |_| true).expect("the sized overlay is drawn");
    assert_eq!(overlay.buffer, (32, 24));
    assert_eq!(overlay.pixel(6, 6), BGRA_GREEN);
    assert_eq!(overlay.pixel(20, 20), BGRA_CLEAR);

    host.handle(HostEvent::Message {
        channel: "embed-test.overlay".to_string(),
        payload: "remove".to_string(),
    });
    let closed = (0..10).any(|_| tick(&mut host).0.contains(&SurfaceCommand::Close(id)));
    assert!(closed, "a removed overlay is closed");
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
    let mut host = new_host(32, 32, || color_probe("embed-test.serve"));
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
fn serving_announces_a_window_before_drawing_it() {
    let _gpu = gpu_lock();
    let mut host = new_host(32, 32, || window_probe("embed-test.serve-window"));
    let (sender, events) = mpsc::channel();
    let mut writer = ClosingWriter {
        bytes: Vec::new(),
        close: sender,
    };

    serve(&mut host, &events, &mut writer).expect("the loop ends when the host closes");

    assert_eq!(message_kinds(&writer.bytes), vec![0x05, 0x02, 0x02]);
}

#[test]
fn a_hidden_surface_draws_nothing_until_shown() {
    let _gpu = gpu_lock();
    let mut host = new_host(16, 16, || color_probe("embed-test.visibility"));
    host.handle(surface(PRIMARY_SURFACE, SurfaceEvent::Visibility(false)));
    assert!(!host.can_draw());

    let (sender, events) = mpsc::channel();
    sender
        .send(LoopEvent::Host(surface(
            PRIMARY_SURFACE,
            SurfaceEvent::Visibility(true),
        )))
        .expect("the loop's channel is open");
    let mut writer = ClosingWriter {
        bytes: Vec::new(),
        close: sender,
    };
    serve(&mut host, &events, &mut writer).expect("the loop ends when the host closes");

    assert!(host.can_draw());
    assert_eq!(message_kinds(&writer.bytes), vec![0x02]);
}

#[test]
fn a_disconnected_host_ends_the_loop() {
    let _gpu = gpu_lock();
    let mut host = new_host(16, 16, || color_probe("embed-test.disconnect"));
    let (sender, events) = mpsc::channel();
    sender
        .send(LoopEvent::Disconnected)
        .expect("the loop's channel is open");

    let mut sink = Vec::new();
    serve(&mut host, &events, &mut sink).expect("a disconnect ends the loop cleanly");
    assert!(sink.is_empty());
}
