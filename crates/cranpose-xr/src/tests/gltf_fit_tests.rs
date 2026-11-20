// How a model fits the box of its volume.

use cranpose::space::Vec3;

use super::*;
use crate::{gltf_import::import, spatial_gltf_model_source::SpatialGltfModelSource};

// The tower: its box at rest is 1 by 2 by 0.5 meters, centered at (0, 1, -2).
fn tower() -> GltfModel {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/models/tower.glb");
    import(&SpatialGltfModelSource::from_path(path)).expect("the tower loads")
}

fn near(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-2, "{a:?} is not {b:?}");
}

fn size_near(a: [f32; 3], b: [f32; 3]) {
    assert!(
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-2),
        "{a:?} is not {b:?}"
    );
}

#[test]
fn a_model_with_no_limit_has_a_meter_for_each_gltf_unit() {
    let fit = ModelFit::new(&tower(), [None; 3]);
    size_near(fit.size, [1000.0, 2000.0, 500.0]);
    // The middle goes to the box's middle. The scene's y is down and its z is away, so the
    // tower's top corner, which is high and near the viewer in glTF, is up and near here.
    near(
        fit.root.transform_point(Vec3::new(0.0, 1.0, -2.0)),
        Vec3::ZERO,
    );
    near(
        fit.root.transform_point(Vec3::new(-0.5, 2.0, -1.75)),
        Vec3::new(-500.0, -1000.0, -250.0),
    );
}

#[test]
fn a_model_scales_to_the_tightest_side_and_keeps_its_proportions() {
    // The ratios are 300/1000, 300/2000 and 300/500: the height is the limit
    let fit = ModelFit::new(&tower(), [Some(300.0); 3]);
    size_near(fit.size, [300.0; 3]);
    // The tower is 2 meters high and fills the box's 300 dp, so a meter is 150 dp
    near(
        fit.root.transform_point(Vec3::new(0.5, 0.0, -2.25)),
        Vec3::new(75.0, 150.0, 37.5),
    );
}

#[test]
fn a_small_model_scales_up_to_fill_a_box_that_the_modifier_sets_on_every_side() {
    let fit = ModelFit::new(&tower(), [Some(4000.0); 3]);
    size_near(fit.size, [4000.0; 3]);
    // The ratios are 4, 2 and 8: the height scales by 2
    near(
        fit.root.transform_point(Vec3::new(0.5, 2.0, -1.75)),
        Vec3::new(1000.0, -2000.0, -500.0),
    );
}

#[test]
fn a_side_with_no_limit_takes_the_scaled_side_and_never_grows_the_model() {
    // The width limit scales by a half, and the other sides follow
    let fit = ModelFit::new(&tower(), [Some(500.0), None, None]);
    size_near(fit.size, [500.0, 1000.0, 250.0]);
    // A wide limit alone leaves the model at its own size, in a box that is wide
    let wide = ModelFit::new(&tower(), [Some(5000.0), None, None]);
    size_near(wide.size, [5000.0, 2000.0, 500.0]);
}
