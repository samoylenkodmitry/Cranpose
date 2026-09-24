use super::*;

#[test]
fn slider_strain_uses_the_shared_incompressible_pose() {
    let pose = crate::dynamics::LiquidPose {
        stretch: 1.05,
        ortho: 1.0 / 1.05,
        ..Default::default()
    };
    let deformation = slider_deformation(pose);
    assert!((deformation.along() - pose.stretch).abs() < 1e-6);
    assert!((deformation.along() * deformation.across() - 1.0).abs() < 1e-6);
}

#[test]
fn slider_strain_preserves_acceleration_axis_compression() {
    let pose = crate::dynamics::LiquidPose {
        stretch: 0.95,
        ortho: 1.0 / 0.95,
        ..Default::default()
    };
    let deformation = slider_deformation(pose);
    assert!((deformation.along() - pose.stretch).abs() < 1e-6);
    assert!(deformation.across() > 1.0);
}
