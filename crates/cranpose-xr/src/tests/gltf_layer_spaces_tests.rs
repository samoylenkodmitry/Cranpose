// The spaces of a model's layer across the evaluations of an animation, as a node that holds the
// space of its last evaluation sees them.

use std::{collections::HashSet, path::PathBuf};

use cranpose::space::Vec3;

use super::*;
use crate::{gltf_import::import, spatial_gltf_model_source::SpatialGltfModelSource};

// The tower, which has one draw, covered three times by one layer: an owner and two more.
fn tower() -> (GltfModel, LayerDraws) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/models/tower.glb");
    let model = import(&SpatialGltfModelSource::from_path(path)).expect("the tower loads");
    let draws = LayerDraws {
        owner: 0,
        more: vec![0, 0],
        spaces: LayerSpaces::new(),
    };
    (model, draws)
}

// The map that puts a node at `x` along the x axis.
fn at(x: f32) -> Mat4 {
    Mat4::translation(Vec3::new(x, 0.0, 0.0))
}

fn transforms(space: &LayerSpace) -> Vec<Mat4> {
    let LayerSpace::Surface(surfaces) = space else {
        panic!("a layer's space holds surfaces");
    };
    surfaces.iter().map(|surface| surface.transform).collect()
}

#[test]
fn an_animation_reuses_a_few_spaces_and_each_one_shows_the_pose_it_was_asked_for() {
    let (model, draws) = tower();
    let mut held = None;
    let mut seen = HashSet::new();
    for frame in 0..60 {
        let x = frame as f32;
        // The node keeps the space of the evaluation before while it takes the next one
        let space = draws.spaces.space((&model, &draws), |_| at(x));
        assert_eq!(transforms(&space), vec![at(x); 3]);
        seen.insert(Arc::as_ptr(&space));
        held = Some(space);
    }
    assert!(held.is_some());
    assert!(
        seen.len() <= SPACES,
        "sixty evaluations made {} spaces",
        seen.len()
    );
}

#[test]
fn a_space_that_a_node_still_holds_never_changes() {
    let (model, draws) = tower();
    let kept = draws.spaces.space((&model, &draws), |_| at(1.0));
    for frame in 2..40 {
        let space = draws.spaces.space((&model, &draws), |_| at(frame as f32));
        assert!(!Arc::ptr_eq(&space, &kept));
    }
    assert_eq!(transforms(&kept), vec![at(1.0); 3]);
}
