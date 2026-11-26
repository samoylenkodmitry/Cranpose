// A skinned mesh whose vertices each follow one joint, cut into a mesh for each joint, and one that
// weighs a vertex between joints, which keeps its bind pose.

use cranpose::space::Vec3;

use crate::{gltf_import::parse, gltf_model::GltfModel};

// A model of the test folder, which the bytes alone describe.
fn load(name: &str) -> GltfModel {
    let file = format!("{}/tests/models/{name}", env!("CARGO_MANIFEST_DIR"));
    parse(&std::fs::read(file).expect("the file"), None).expect("the model")
}

fn near(a: [f32; 3], b: [f32; 3]) {
    assert!(
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6),
        "{a:?} is not {b:?}"
    );
}

// The box of the test controller at rest (scripts/dev/make_gltf_models.py).
const LOW: [f32; 3] = [-0.02, -0.055, -0.05];
const HIGH: [f32; 3] = [0.025, 0.025, 0.05];

#[test]
fn a_rigid_skin_hangs_one_mesh_on_each_joint_and_keeps_the_box_of_its_bind_pose() {
    let model = load("controller_parts.glb");
    let nodes: Vec<usize> = model.draws.iter().map(|draw| draw.node).collect();
    // The body and the six parts are the joints, nodes 1 to 7
    assert_eq!(nodes, [1, 2, 3, 4, 5, 6, 7]);
    assert!(model.draws.iter().all(|draw| draw.layer == 0));
    near(model.bounds.0, LOW);
    near(model.bounds.1, HIGH);
    // The trigger's lever lies in the trigger's space: it hangs 3 cm down from the joint's origin
    let trigger = &model.draws[1];
    for &[x, y, z] in trigger.mesh.positions() {
        assert!(x.abs() <= 0.005 + 1e-6 && z.abs() <= 0.005 + 1e-6);
        assert!((-0.03 - 1e-6..=1e-6).contains(&y), "{y}");
    }
}

#[test]
fn a_joint_that_moves_moves_its_piece_and_no_other() {
    let model = load("controller_parts.glb");
    let (mut locals, mut worlds) = (Vec::new(), Vec::new());
    model.local_transforms([], &mut locals);
    // The trigger's joint goes 1 cm down
    locals[2].translation = [0.0, -0.03, -0.04];
    model.worlds_of(&locals, &mut worlds);
    let top = |draw: usize| {
        model.draws[draw]
            .mesh
            .positions()
            .iter()
            .map(|&position| {
                worlds[model.draws[draw].node]
                    .transform_point(Vec3::from_array(position))
                    .y
            })
            .fold(f32::NEG_INFINITY, f32::max)
    };
    // The trigger's lever hangs from its joint, now 3 cm below the body's middle
    assert!((top(1) - -0.03).abs() < 1e-6);
    // The first button stands where it stood
    assert!((top(3) - 0.025).abs() < 1e-6);
}

#[test]
fn a_skin_that_weighs_a_vertex_between_joints_keeps_one_mesh_in_its_bind_pose() {
    let model = load("controller_blended.glb");
    let nodes: Vec<usize> = model.draws.iter().map(|draw| draw.node).collect();
    // The mesh's own node, which stands at the root
    assert_eq!(nodes, [8]);
    near(model.bounds.0, LOW);
    near(model.bounds.1, HIGH);
    // Its parts still move, with nothing on their nodes to show it
    assert!(!model.parts.is_empty());
}
