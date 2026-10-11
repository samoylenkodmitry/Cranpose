use std::sync::{Arc, Mutex};

use cranpose_ui_graphics::{
    BRUSH_KIND_LINEAR, BlendMode, FRAGMENT_KIND_FILL, RecordLane, RecordSegment,
};

use super::{KeyedBuild, ShapePipelines};
use crate::{
    ablation::ShapeAblation,
    pipeline_compiler::PipelineCompiler,
    pipeline_recorder::PipelineRecorder,
    render::{RunTier, SegmentClip, ShapeDepth, ShapePipelineKey, ShapeTurns, ShapeVariant},
};

/// Builds each key as itself, listing the keys in the order built.
struct ListingBuilder(Arc<Mutex<Vec<ShapePipelineKey>>>);

impl KeyedBuild for ListingBuilder {
    type Output = ShapePipelineKey;

    fn build(&self, key: ShapePipelineKey) -> ShapePipelineKey {
        self.0.lock().expect("built keys").push(key);
        key
    }
}

fn pipelines() -> (
    ShapePipelines<ListingBuilder>,
    Arc<Mutex<Vec<ShapePipelineKey>>>,
) {
    let built = Arc::new(Mutex::new(Vec::new()));
    let pipelines = ShapePipelines::new(
        ListingBuilder(Arc::clone(&built)),
        wgpu::Backend::Vulkan,
        &PipelineCompiler::spawn(),
        [],
        PipelineRecorder::default(),
    );
    (pipelines, built)
}

/// A rect fill's key in `tier`, plain or with a gradient: specialized keys
/// one general pipeline stands in for.
fn fill(tier: RunTier, gradient: bool) -> ShapePipelineKey {
    let segment = RecordSegment {
        lane: RecordLane::Shapes,
        start: 0,
        count: 1,
        blend: BlendMode::SrcOver,
        gradient,
        vertex_gradient: false,
        brushes: if gradient { 1 << BRUSH_KIND_LINEAR } else { 1 },
        kinds: 1 << FRAGMENT_KIND_FILL,
        band_class: 0,
        interiors: false,
        occluders: false,
        bare_interiors: false,
    };
    ShapePipelineKey {
        blend_mode: BlendMode::SrcOver,
        tier,
        variant: ShapeVariant::of_segment(
            &segment,
            SegmentClip::Tested,
            ShapeAblation::default(),
            false,
            (true, true),
        ),
        turns: ShapeTurns::None,
        depth: ShapeDepth::Off,
    }
}

fn built(list: &Mutex<Vec<ShapePipelineKey>>) -> Vec<ShapePipelineKey> {
    list.lock().expect("built keys").clone()
}

#[test]
fn draws_build_their_own_pipelines_until_a_general_one_would_pay() {
    let (mut pipelines, list) = pipelines();
    let keys = [
        fill(RunTier::Store, false),
        fill(RunTier::Arena, false),
        fill(RunTier::Store, true),
        fill(RunTier::Arena, true),
    ];
    let general = keys[0].general();
    assert!(
        keys.iter()
            .all(|key| key.general() == general && *key != general),
        "the keys share one general pipeline: {keys:?}"
    );

    // A fresh install: no general is built, and building one would cost a
    // draw more than its own pipeline.
    pipelines.ensure(keys[0], 6);
    pipelines.ensure(keys[1], 6);
    assert_eq!(built(&list), keys[..2]);
    assert_eq!(pipelines.get(keys[1]), Some((&keys[1], false)));

    // A third draws: the general now stands in for it and every later one.
    pipelines.ensure(keys[2], 6);
    assert_eq!(built(&list), [keys[0], keys[1], general]);
    assert_eq!(pipelines.get(keys[2]), Some((&general, true)));
    pipelines.ensure(keys[3], 6);
    assert_eq!(built(&list).len(), 3, "the built general stands in at once");
    assert_eq!(pipelines.get(keys[3]), Some((&general, true)));
}
