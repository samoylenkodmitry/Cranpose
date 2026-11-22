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

// The words of a tray of `size` dp at `position`, and the tray's volume under them
fn tray(id: u64, size: [f32; 2], position: [f32; 3]) -> [PanelSlot; 2] {
    let words = PanelSlot {
        id,
        kind: PanelKind::Content,
        atlas: [0.0, 0.0, size[0], size[1]],
        pose: Pose::at(position),
        mobility: crate::space::Mobility::Fixed,
        shape: crate::space::Shape::Flat,
        resize: None,
        room: size,
        priority: 0,
        ride: Ride::Free,
        tracked: None,
    };
    let volume = PanelSlot {
        id: id + 1,
        kind: PanelKind::Volume {
            depth: 100.0,
            under: Some(id),
        },
        ..words
    };
    [words, volume]
}

const HEAD: Option<[f32; 3]> = Some([0.0, 0.0, 0.0]);

#[test]
fn the_words_of_a_tray_skip_the_depth_test_and_other_panels_keep_it() {
    let [words, volume] = tray(1, [800.0, 500.0], [0.0, 0.0, -1.0]);
    let plain = PanelSlot {
        id: 9,
        kind: PanelKind::Content,
        ..words
    };
    let panels = [plain, words, volume];
    assert!(!skips_depth_test(&panels, 0, HEAD));
    assert!(skips_depth_test(&panels, 1, HEAD));
    assert!(skips_depth_test(&panels, 1, None));
}

#[test]
fn a_tray_in_front_covers_the_words_of_a_tray_behind_it() {
    let [far, far_volume] = tray(1, [800.0, 500.0], [0.0, 0.0, -1.0]);
    let [near, near_volume] = tray(3, [400.0, 300.0], [0.1, -0.1, -0.6]);
    let panels = [far, far_volume, near, near_volume];
    assert!(
        !skips_depth_test(&panels, 0, HEAD),
        "the near tray covers the far words"
    );
    assert!(
        skips_depth_test(&panels, 2, HEAD),
        "nothing stands in front of the near words"
    );
}

#[test]
fn a_tray_beside_the_line_of_sight_or_hidden_covers_nothing() {
    let [far, far_volume] = tray(1, [800.0, 500.0], [0.0, 0.0, -1.0]);
    let [near, near_volume] = tray(3, [400.0, 300.0], [0.6, 0.0, -0.6]);
    assert!(skips_depth_test(
        &[far, far_volume, near, near_volume],
        0,
        HEAD
    ));
    let [near, near_volume] = tray(3, [400.0, 300.0], [0.0, 0.0, -0.6]);
    let hidden = PanelSlot {
        ride: Ride::Hidden,
        ..near_volume
    };
    assert!(skips_depth_test(&[far, far_volume, near, hidden], 0, HEAD));
    assert!(!skips_depth_test(
        &[far, far_volume, near, near_volume],
        0,
        HEAD
    ));
}
