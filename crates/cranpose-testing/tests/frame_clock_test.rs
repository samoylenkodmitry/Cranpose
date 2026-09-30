use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose_app_shell::AppShell;
use cranpose_core::{
    LaunchedEffectAsync, MutableState, location_key, remember, rememberMutableStateOf,
};
use cranpose_render_common::graph::{LayerNode, PrimitiveNode, RenderNode};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, FocusRequester, Modifier, SemanticsNode,
    SemanticsRole, Text, TextFieldState, TextStyle,
};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

type FrameTimes = Rc<RefCell<Vec<u64>>>;
type InputState = Rc<RefCell<Option<MutableState<u32>>>>;

struct CounterApp {
    shell: AppShell<TestRenderer>,
    times: FrameTimes,
    input: InputState,
}

impl CounterApp {
    fn new() -> Self {
        let times = FrameTimes::default();
        let input = InputState::default();
        let app_times = times.clone();
        let app_input = input.clone();
        let mut shell = AppShell::new_with_size(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || frame_counter(app_times.clone(), app_input.clone()),
            (320, 100),
            (320.0, 100.0),
        );
        shell.set_semantics_enabled(true);
        Self {
            shell,
            times,
            input,
        }
    }
}

#[cranpose_ui::composable]
fn frame_counter(times: FrameTimes, input: InputState) {
    let frames = rememberMutableStateOf(|| 0u32);
    let input_value = rememberMutableStateOf(|| 0u32);
    *input.borrow_mut() = Some(input_value);
    LaunchedEffectAsync((), move |scope| {
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            while scope.is_active() {
                let time = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                times.borrow_mut().push(time);
                frames.set(frames.get() + 1);
            }
        })
    });
    Text(
        format!("Frames {}; input {}", frames.get(), input_value.get()),
        Modifier::empty(),
        TextStyle::default(),
    );
}

fn text(node: &SemanticsNode) -> Option<&str> {
    if let SemanticsRole::Text { value } = &node.role {
        return Some(value.as_str());
    }
    node.children.iter().find_map(text)
}

fn visible_text(shell: &mut AppShell<TestRenderer>) -> String {
    let semantics = shell.semantics_tree().expect("counter semantics");
    text(semantics.root()).expect("visible counter").to_string()
}

#[test]
fn repeated_updates_with_one_display_timestamp_advance_the_visible_counter_once() {
    let CounterApp {
        mut shell,
        times,
        input,
    } = CounterApp::new();
    shell.update_without_frame();

    shell.update_at_frame_time_nanos(16_666_667);
    assert_eq!(visible_text(&mut shell), "Frames 1; input 0");
    for _ in 0..4 {
        shell.update_at_frame_time_nanos(16_666_667);
    }
    assert_eq!(
        visible_text(&mut shell),
        "Frames 1; input 0",
        "scheduler or redraw wakeups within one display frame must not advance animation"
    );
    assert_eq!(*times.borrow(), [16_666_667]);

    input.borrow().expect("input state").set(7);
    shell.update_at_frame_time_nanos(16_666_667);
    assert_eq!(
        visible_text(&mut shell),
        "Frames 1; input 7",
        "state changes must still render before another display frame"
    );

    shell.update_at_frame_time_nanos(33_333_334);
    assert_eq!(visible_text(&mut shell), "Frames 2; input 7");
    shell.update_at_frame_time_nanos(30_000_000);
    assert_eq!(visible_text(&mut shell), "Frames 2; input 7");
    assert_eq!(*times.borrow(), [16_666_667, 33_333_334]);
}

#[test]
fn platform_frames_run_while_rendering_is_deferred_and_ui_work_does_not_add_ticks() {
    let CounterApp {
        mut shell,
        times,
        input,
    } = CounterApp::new();
    shell.update_without_frame();
    let start = Instant::now();
    shell.dispatch_frame_at(start);
    assert_eq!(
        times.borrow().len(),
        1,
        "frame callbacks do not wait for rendering"
    );
    for _ in 0..4 {
        shell.dispatch_frame_at(start);
        shell.update_without_frame();
    }
    assert_eq!(visible_text(&mut shell), "Frames 1; input 0");
    input.borrow().expect("input state").set(9);
    shell.update_without_frame();
    assert_eq!(visible_text(&mut shell), "Frames 1; input 9");
    for frame in 1..=3 {
        shell.dispatch_frame_at(start + Duration::from_nanos(frame * 16_666_667));
    }
    shell.update_without_frame();
    assert_eq!(visible_text(&mut shell), "Frames 4; input 9");
    assert!(
        times
            .borrow()
            .windows(2)
            .all(|pair| pair[1] - pair[0] == 16_666_667)
    );

    let resumed = start + Duration::from_secs(3);
    shell.notify_app_paused();
    shell.update_without_frame();
    shell.notify_app_resumed();
    shell.dispatch_frame_at(resumed);
    shell.update_without_frame();
    assert_eq!(visible_text(&mut shell), "Frames 5; input 9");
    let frames = times.borrow();
    assert_eq!(frames[4] - frames[0], 3_000_000_000);
    assert_eq!(
        shell.realtime_pointer_event_time(None).animation_time_nanos,
        frames[4]
    );
}

#[test]
fn first_zero_timestamp_and_exact_manual_frames_remain_available() {
    let CounterApp {
        mut shell, times, ..
    } = CounterApp::new();
    shell.update_without_frame();
    shell.update_at_frame_time_nanos(0);
    assert_eq!(visible_text(&mut shell), "Frames 1; input 0");
    shell.update_after_exact_interval(Duration::from_nanos(16_666_667));
    shell.update_after_exact_interval(Duration::from_nanos(16_666_667));
    assert_eq!(visible_text(&mut shell), "Frames 3; input 0");
    assert_eq!(*times.borrow(), [0, 16_666_667, 33_333_334]);
    shell.update_at_frame_time_nanos(10_000_000_000);
    shell.update();
    assert_eq!(visible_text(&mut shell), "Frames 4; input 0");
}

fn contains_caret(layer: &LayerNode) -> bool {
    let is_caret = |primitive: &DrawPrimitive| {
        matches!(primitive,
        DrawPrimitive::Rect { brush: Brush::Solid(color), .. } if *color == Color::GREEN)
    };
    layer.children.iter().any(|node| match node {
        RenderNode::Layer(child) => contains_caret(child),
        RenderNode::DrawRun(run) => run.primitives().any(|primitive| is_caret(&primitive)),
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw) => is_caret(&draw.primitive),
            PrimitiveNode::Text(_) => false,
        },
    })
}

#[test]
fn wall_time_cursor_blink_renders_without_an_animation_callback() {
    let requester = FocusRequester::new();
    let content_requester = requester.clone();
    let mut shell = AppShell::new_with_size(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let state = remember(|| TextFieldState::new("Caret")).with(|state| *state);
            BasicTextFieldWithOptions(
                state,
                Modifier::empty()
                    .width(200.0)
                    .height(40.0)
                    .focus_requester(&content_requester),
                BasicTextFieldOptions {
                    cursor_color: Color::GREEN,
                    show_keyboard_on_focus: false,
                    ..Default::default()
                },
            );
        },
        (320, 100),
        (320.0, 100.0),
    );
    shell.update_without_frame();
    requester.request_focus().expect("focus editable field");
    for _ in 0..3 {
        shell.update_without_frame();
    }
    assert!(contains_caret(
        &shell.scene().graph.as_ref().expect("field scene").root
    ));
    let deadline = shell.next_event_time().expect("next cursor transition");
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(5),
    );
    if shell.needs_update_without_frame() {
        shell.update_without_frame();
    }
    assert!(
        !contains_caret(&shell.scene().graph.as_ref().expect("field scene").root),
        "the caret must disappear at its wall-time deadline without a display callback"
    );
}
