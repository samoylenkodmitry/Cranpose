// The spaces of a layer across the evaluations of a moving model, as a node that holds the space of
// its last evaluation sees them.

use cranpose::space::Vec3;

use super::*;

// A layer that shows one mesh three times: an owner and two more.
fn meshes() -> Vec<Arc<Mesh>> {
    vec![Arc::new(Mesh::plane()); 3]
}

// The map that puts a surface at `x` along the x axis.
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
fn a_moving_model_reuses_a_few_spaces_and_each_one_shows_the_pose_it_was_asked_for() {
    let (ring, meshes) = (SurfaceRing::new(), meshes());
    let mut held = None;
    let mut seen = Vec::new();
    for frame in 0..60 {
        let x = frame as f32;
        // The node keeps the space of the evaluation before while it takes the next one
        let space = ring.space(&meshes, |_| at(x));
        assert_eq!(transforms(&space), vec![at(x); 3]);
        if !seen
            .iter()
            .any(|known| std::ptr::eq(*known, Arc::as_ptr(&space)))
        {
            seen.push(Arc::as_ptr(&space));
        }
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
fn each_surface_takes_the_place_of_its_own_index() {
    let (ring, meshes) = (SurfaceRing::new(), meshes());
    let space = ring.space(&meshes, |index| at(index as f32 * 10.0));
    assert_eq!(transforms(&space), vec![at(0.0), at(10.0), at(20.0)]);
    drop(space);
    let again = ring.space(&meshes, |index| at(index as f32 * 5.0));
    assert_eq!(transforms(&again), vec![at(0.0), at(5.0), at(10.0)]);
}

#[test]
fn a_space_that_a_node_still_holds_never_changes() {
    let (ring, meshes) = (SurfaceRing::new(), meshes());
    let kept = ring.space(&meshes, |_| at(1.0));
    for frame in 2..40 {
        let space = ring.space(&meshes, |_| at(frame as f32));
        assert!(!Arc::ptr_eq(&space, &kept));
    }
    assert_eq!(transforms(&kept), vec![at(1.0); 3]);
}
