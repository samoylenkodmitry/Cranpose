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
        &PipelineCompiler::spawn(wgpu::Backend::Vulkan),
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

/// Builds each key as itself, as a browser compiling in the background
/// does: a pipeline draws once `landed` lists its key.
struct BrowserBuilder {
    built: KeyList,
    landed: KeyList,
}

impl KeyedBuild for BrowserBuilder {
    type Output = (ShapePipelineKey, KeyList);

    fn build(&self, key: ShapePipelineKey) -> Self::Output {
        self.built.lock().expect("built keys").push(key);
        (key, Arc::clone(&self.landed))
    }

    fn landed((key, landed): &Self::Output) -> bool {
        landed.lock().expect("landed keys").contains(key)
    }
}

/// Keys a test lists, shared with the builder that lists them.
type KeyList = Arc<Mutex<Vec<ShapePipelineKey>>>;

/// Pipelines that stand in as in a browser, with the keys built and the
/// keys whose pipelines landed.
fn browser_pipelines() -> (ShapePipelines<BrowserBuilder>, KeyList, KeyList) {
    let built = Arc::new(Mutex::new(Vec::new()));
    let landed = Arc::new(Mutex::new(Vec::new()));
    let pipelines = ShapePipelines::new(
        BrowserBuilder {
            built: Arc::clone(&built),
            landed: Arc::clone(&landed),
        },
        wgpu::Backend::BrowserWebGpu,
        &PipelineCompiler::spawn(wgpu::Backend::BrowserWebGpu),
        [],
        PipelineRecorder::default(),
    );
    (pipelines, built, landed)
}

#[test]
fn browser_draws_take_the_universal_pipeline_until_their_own_compiled_in_the_background() {
    let (mut pipelines, list, landed) = browser_pipelines();
    let plain = fill(RunTier::Arena, false);
    let gradient = fill(RunTier::Arena, true);
    let general = plain.universal();
    assert_eq!(general, gradient.universal());

    pipelines.ensure(plain, 4);
    pipelines.ensure(gradient, 4);
    pipelines.request_wanted();
    assert_eq!(
        built(&list),
        vec![general],
        "the first screen compiles one universal pipeline for both"
    );
    let drawn = |pipelines: &ShapePipelines<BrowserBuilder>, key| {
        pipelines
            .get(key)
            .map(|((built, _), fallback)| (*built, fallback))
    };
    assert_eq!(drawn(&pipelines, plain), Some((general, true)));

    pipelines.begin_frame();
    pipelines.ensure(plain, 4);
    pipelines.ensure(gradient, 4);
    pipelines.build_in_background();
    pipelines.request_wanted();
    assert_eq!(
        built(&list),
        vec![general, plain, gradient],
        "once the browser compiles in the background, each specialized one builds"
    );
    assert_eq!(
        drawn(&pipelines, plain),
        Some((general, true)),
        "until it compiled, the universal one draws"
    );

    landed.lock().expect("landed keys").push(plain);
    assert_eq!(drawn(&pipelines, plain), Some((plain, false)));
    assert_eq!(drawn(&pipelines, gradient), Some((general, true)));
}

#[test]
fn a_browser_builds_the_universal_pipeline_for_a_recorded_first_screen() {
    let (mut pipelines, list, _) = browser_pipelines();
    let plain = fill(RunTier::Arena, false);
    pipelines.build_ahead(plain);
    assert_eq!(built(&list), vec![plain.universal()]);
}
