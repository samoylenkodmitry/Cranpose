use super::*;

// The name that Jetpack XR gives a shape that Meta spells with one letter for the side.
fn jetpack_name_of(meta: &str) -> String {
    for (short, long) in [
        ("Lb", "LeftBottom"),
        ("Lt", "LeftTop"),
        ("Rb", "RightBottom"),
        ("Rt", "RightTop"),
        ("L", "Left"),
        ("R", "Right"),
    ] {
        // Only the end of the name carries the side. `EyesLookLeftL` keeps its `Left`.
        if let Some(stem) = meta.strip_suffix(short) {
            if stem.ends_with(|c: char| c.is_ascii_lowercase()) {
                return format!("{stem}{long}");
            }
        }
    }
    meta.to_owned()
}

#[test]
fn the_shapes_that_both_runtimes_share_map_by_name_in_both_directions() {
    let mut mapped = 0;
    for expression in FaceExpression::ALL {
        let Some(shape) = expression.blend_shape() else {
            continue;
        };
        mapped += 1;
        let meta = format!("{expression:?}");
        let jetpack = format!("{shape:?}");
        let expected = match meta.as_str() {
            // The two names that do not end in a side letter
            "ChinRaiserB" => "ChinRaiserBottom".to_owned(),
            "ChinRaiserT" => "ChinRaiserTop".to_owned(),
            _ => jetpack_name_of(&meta),
        };
        assert_eq!(jetpack, expected, "{meta}");
        assert_eq!(shape.expression(), Some(expression), "{jetpack}");
    }
    assert_eq!(mapped, 64, "63 shared shapes and the tongue's out");
}

#[test]
fn the_tongue_shapes_that_only_one_runtime_has_map_to_nothing() {
    for expression in [
        FaceExpression::TongueTipInterdental,
        FaceExpression::TongueTipAlveolar,
        FaceExpression::TongueFrontDorsalPalate,
        FaceExpression::TongueMidDorsalPalate,
        FaceExpression::TongueBackDorsalVelar,
        FaceExpression::TongueRetreat,
    ] {
        assert_eq!(expression.blend_shape(), None, "{expression:?}");
    }
    for shape in [
        FaceBlendShapeType::TongueLeft,
        FaceBlendShapeType::TongueRight,
        FaceBlendShapeType::TongueUp,
        FaceBlendShapeType::TongueDown,
    ] {
        assert_eq!(shape.expression(), None, "{shape:?}");
    }
    assert_eq!(
        FaceExpression::TongueOut.blend_shape(),
        Some(FaceBlendShapeType::TongueOut)
    );
}

#[test]
fn the_weights_of_metas_tracker_land_on_the_shapes_with_the_same_name() {
    let mut meta = MetaFaceState {
        is_valid: true,
        lower_face_confidence: 0.25,
        upper_face_confidence: 0.75,
        ..MetaFaceState::default()
    };
    meta.weights[FaceExpression::JawDrop.index()] = 0.6;
    meta.weights[FaceExpression::LipFunnelerRb.index()] = 0.3;
    meta.weights[FaceExpression::EyesLookLeftR.index()] = 0.1;
    meta.weights[FaceExpression::TongueOut.index()] = 0.9;
    meta.weights[FaceExpression::TongueRetreat.index()] = 1.0;
    let face = FaceState::from_meta(&meta);
    assert_eq!(face.tracking_state, TrackingState::Tracking);
    assert!(face.is_valid);
    assert_eq!(face.weight(FaceBlendShapeType::JawDrop), 0.6);
    assert_eq!(face.weight(FaceBlendShapeType::LipFunnelerRightBottom), 0.3);
    assert_eq!(face.weight(FaceBlendShapeType::EyesLookLeftRight), 0.1);
    assert_eq!(face.weight(FaceBlendShapeType::TongueOut), 0.9);
    // The tongue shapes that only Android XR has stay at rest, and Meta's own have no place
    assert_eq!(face.weight(FaceBlendShapeType::TongueLeft), 0.0);
    assert_eq!(face.blend_shapes.iter().filter(|w| **w != 0.0).count(), 4);
    assert_eq!(face.confidence_regions, [0.25, 0.75, 0.75]);
}

#[test]
fn a_face_that_metas_tracker_reports_invalid_is_paused_with_every_weight_at_rest() {
    let face = FaceState::from_meta(&MetaFaceState::default());
    assert_eq!(face.tracking_state, TrackingState::Paused);
    assert!(!face.is_valid);
    assert!(face.blend_shapes.iter().all(|weight| *weight == 0.0));
    assert_eq!(FaceState::default().tracking_state, TrackingState::Stopped);
}
