use cranpose_liquid::prelude::{
    LiquidSegmentedControl, LiquidSlider, LiquidTheme, LiquidThemeSpec, LiquidToggle,
};
use cranpose_render_common::graph::{LayerNode, RenderNode};

use super::*;

const FRAME_NANOS: u64 = 16_666_667;
const SETTLE_FRAME_LIMIT: usize = 240;

thread_local! {
    static LENS_MOVED: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
    static SLIDER_VALUE: Cell<Option<MutableState<f32>>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LensControl {
    Toggle,
    Segmented,
    Slider,
}

#[composable]
fn LensHost(control: LensControl) {
    let moved = rememberMutableStateOf(|| false);
    LENS_MOVED.with(|slot| *slot.borrow_mut() = Some(moved));
    let far = moved.get();
    LiquidTheme(LiquidThemeSpec::default(), move || match control {
        LensControl::Toggle => LiquidToggle(Modifier::empty(), far, |_| {}),
        LensControl::Slider => LiquidSlider(
            Modifier::empty().width(240.0),
            if far { 1.0 } else { 0.0 },
            |_| {},
        ),
        LensControl::Segmented => LiquidSegmentedControl(
            Modifier::empty().width(240.0),
            if far { 2 } else { 0 },
            |_| {},
            |scope| {
                for label in ["Day", "Week", "Month"] {
                    scope.segment(label);
                }
            },
        ),
    });
}

fn lens_layers(shell: &AppShell<ScopedUpdateCountingRenderer>) -> Vec<GraphicsLayer> {
    fn collect(layer: &LayerNode, out: &mut Vec<GraphicsLayer>) {
        if layer.graphics_layer.backdrop_effect.is_some() {
            out.push(GraphicsLayer {
                translation_x: 0.0,
                ..layer.graphics_layer.clone()
            });
        }
        for child in &layer.children {
            if let RenderNode::Layer(child) = child {
                collect(child, out);
            }
        }
    }
    let mut layers = Vec::new();
    if let Some(graph) = shell.surfaces[0].renderer.scene().graph.as_ref() {
        collect(&graph.root, &mut layers);
    }
    layers
}

fn run_until_idle(host: &mut CountedShell, clock: &mut u64, what: &str) -> usize {
    for frame in 1..=SETTLE_FRAME_LIMIT {
        *clock += FRAME_NANOS;
        host.shell.update_at_frame_time_nanos(*clock);
        assert_scene_matches_rebuild(&mut host.shell, &format!("{what}, frame {frame}"));
        if !host.shell.needs_update() {
            return frame;
        }
    }
    panic!("{what}: still asking for frames after {SETTLE_FRAME_LIMIT}");
}

#[test]
fn a_moved_liquid_lens_redraws_itself_until_it_rests_and_then_stops() {
    let _guard = test_guard();
    for control in [
        LensControl::Toggle,
        LensControl::Segmented,
        LensControl::Slider,
    ] {
        LENS_MOVED.with(|slot| slot.borrow_mut().take());
        let mut host =
            CountedShell::settled(location_key(file!(), line!(), column!()), move || {
                LensHost(control);
            });
        let moved = LENS_MOVED
            .with(|slot| *slot.borrow())
            .expect("the host exposes its lens state");
        let mut clock = host.shell.app.last_frame_time_nanos + FRAME_NANOS;
        run_until_idle(&mut host, &mut clock, &format!("{control:?} settling"));
        let rest = lens_layers(&host.shell);
        assert!(!rest.is_empty(), "{control:?} should draw a glass lens");

        moved.set_value(true);
        let frames = run_until_idle(&mut host, &mut clock, &format!("{control:?} lens travel"));
        assert!(
            frames > 10,
            "{control:?}: the lens should travel for a while, not {frames} frames"
        );
        assert_eq!(
            lens_layers(&host.shell),
            rest,
            "{control:?}: once frames stop the lens must be back at rest, not frozen \
             in the pose it had when nothing else drew it again"
        );

        clock += FRAME_NANOS;
        let settled = host.shell.update_at_frame_time_nanos(clock);
        assert!(
            !settled.content_redrawn,
            "{control:?}: a lens at rest must not draw again"
        );
        assert!(
            !host.shell.needs_update(),
            "{control:?}: a lens at rest must not ask for frames"
        );
    }
}

#[composable]
fn SliderReleaseHost(accept: bool) {
    let value = rememberMutableStateOf(|| 0.5);
    SLIDER_VALUE.set(Some(value));
    LiquidTheme(LiquidThemeSpec::default(), move || {
        LiquidSlider(Modifier::empty().width(300.0), value.get(), move |next| {
            if accept {
                value.set(next);
            }
        });
    });
}

#[test]
fn a_released_slider_streams_native_inertia_then_stops_requesting_frames() {
    let _guard = test_guard();
    for accept in [true, false] {
        let mut host =
            CountedShell::settled(location_key(file!(), line!(), column!()), move || {
                SliderReleaseHost(accept);
            });
        let value = SLIDER_VALUE.get().expect("controlled slider");
        let mut clock = host.shell.app.last_frame_time_nanos + FRAME_NANOS;
        run_until_idle(&mut host, &mut clock, "slider rest");
        let rest = drawn_picture(&host.shell);
        let origin = clock;
        let scene = host.shell.surfaces[0].renderer.scene_mut();
        let graph = scene.graph.take().expect("slider scene");
        collect_graph_hits(
            &graph.root,
            cranpose_render_common::graph::ProjectiveTransform::identity(),
            scene,
            None,
        );
        scene.replace_graph(graph);
        host.shell.set_cursor_at_event_time(
            150.0,
            16.0,
            PointerEventTime {
                platform_time_ms: Some(0),
                animation_time_nanos: origin,
            },
        );
        assert!(host.shell.pointer_pressed_at_event_time(PointerEventTime {
            platform_time_ms: Some(0),
            animation_time_nanos: origin,
        }));
        for (ms, x) in [(600, 217.0), (610, 221.0), (620, 225.0), (630, 229.0)] {
            clock = origin + ms * 1_000_000;
            host.shell.set_cursor_at_event_time(
                x,
                16.0,
                PointerEventTime {
                    platform_time_ms: Some(ms as i64),
                    animation_time_nanos: clock,
                },
            );
            host.shell.update_at_frame_time_nanos(clock);
        }
        assert!((value.get() - if accept { 0.73 } else { 0.5 }).abs() < 0.0001);
        clock += 800_000_000;
        host.shell.update_at_frame_time_nanos(clock);
        assert!(host.shell.pointer_released_at_event_time(PointerEventTime {
            platform_time_ms: Some(1430),
            animation_time_nanos: clock,
        }));
        let frames = run_until_idle(&mut host, &mut clock, "slider inertia");
        assert!(
            frames > 10,
            "native release must keep moving after pointer-up"
        );
        assert!(
            (value.get() - if accept { 0.862 } else { 0.5 }).abs() < 0.0001,
            "released value {}",
            value.get()
        );
        let settled = value.get();
        for _ in 0..12 {
            clock += FRAME_NANOS;
            assert!(!host.shell.update_at_frame_time_nanos(clock).content_redrawn);
            assert!(!host.shell.needs_update());
            assert_eq!(value.get(), settled);
        }
        if !accept {
            assert_eq!(
                drawn_picture(&host.shell),
                rest,
                "the declined value must return to its controlled pose"
            );
        }
    }
}
