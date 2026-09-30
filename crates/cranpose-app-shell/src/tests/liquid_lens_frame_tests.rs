use cranpose_liquid::prelude::{
    LiquidSegmentedControl, LiquidSlider, LiquidTheme, LiquidThemeSpec, LiquidToggle,
};
use cranpose_render_common::graph::{LayerNode, RenderNode};

use super::*;

const FRAME_NANOS: u64 = 16_666_667;
const SETTLE_FRAME_LIMIT: usize = 240;

thread_local! {
    static LENS_MOVED: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
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
