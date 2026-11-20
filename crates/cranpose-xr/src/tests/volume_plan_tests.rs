use super::*;

const BOTH: [bool; 2] = [true, true];
const NEITHER: DepthSupport = DepthSupport {
    submits: false,
    tests: false,
};
const SUBMITS: DepthSupport = DepthSupport {
    submits: true,
    tests: false,
};
const FULL: DepthSupport = DepthSupport {
    submits: true,
    tests: true,
};

#[test]
fn a_frame_with_no_volume_has_no_layer() {
    assert_eq!(plan(0, BOTH, FULL), None);
}

#[test]
fn a_frame_needs_both_eyes_for_the_layer() {
    assert_eq!(plan(2, [true, false], FULL), None);
    assert_eq!(plan(2, [false, true], FULL), None);
    assert_eq!(plan(2, [false, false], FULL), None);
}

#[test]
fn volumes_make_one_layer_however_many_there_are() {
    assert_eq!(plan(1, BOTH, NEITHER), plan(5, BOTH, NEITHER));
    assert!(plan(1, BOTH, NEITHER).is_some());
}

#[test]
fn the_layer_carries_depth_only_when_the_runtime_takes_it() {
    let plan = |depth| plan(1, BOTH, depth).expect("a layer");
    assert!(!plan(NEITHER).submits_depth);
    assert!(plan(SUBMITS).submits_depth);
    assert!(plan(FULL).submits_depth);
}

#[test]
fn layers_over_the_projection_test_its_depth_only_with_both_extensions() {
    let plan = |depth| plan(1, BOTH, depth);
    assert!(!plan(NEITHER).expect("a layer").tests_depth);
    assert!(!plan(SUBMITS).expect("a layer").tests_depth);
    assert!(plan(FULL).expect("a layer").tests_depth);
    // The test with no depth to read does nothing
    let tests_only = DepthSupport {
        submits: false,
        tests: true,
    };
    assert!(!plan(tests_only).expect("a layer").tests_depth);
}

#[test]
fn the_compositor_draws_streams_then_volumes_then_panels_then_rays() {
    assert_eq!(
        STAGES,
        [
            Stage::Streams,
            Stage::Volumes,
            Stage::Panels,
            Stage::Overlay
        ]
    );
}

#[test]
fn panels_and_rays_test_depth_and_the_projection_and_the_streams_do_not() {
    let tested: Vec<_> = STAGES
        .into_iter()
        .filter(|stage| tests_depth(*stage, plan(1, BOTH, FULL)))
        .collect();
    assert_eq!(tested, [Stage::Panels, Stage::Overlay]);
    for stage in STAGES {
        assert!(!tests_depth(stage, plan(1, BOTH, SUBMITS)));
        assert!(!tests_depth(stage, plan(0, BOTH, FULL)));
        assert!(!tests_depth(stage, None));
    }
}

#[test]
fn only_the_projection_layer_writes_depth_and_only_where_layers_test_it() {
    let written: Vec<_> = STAGES
        .into_iter()
        .filter(|stage| writes_depth(*stage, plan(1, BOTH, FULL)))
        .collect();
    assert_eq!(written, [Stage::Volumes]);
    for stage in STAGES {
        // With no test over it, a write has no reader
        assert!(!writes_depth(stage, plan(1, BOTH, SUBMITS)));
        assert!(!writes_depth(stage, plan(0, BOTH, FULL)));
        assert!(!writes_depth(stage, None));
    }
}
