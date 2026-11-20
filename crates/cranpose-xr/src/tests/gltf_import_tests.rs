// The loader on the hand-made files of `tests/models`, and on files that are not models.

use std::path::PathBuf;

use cranpose::space::Vec3;

use super::*;
use crate::gltf_model::LayerContent;

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/models")
        .join(name)
}

fn load(name: &str) -> Result<GltfModel, GltfError> {
    import(&SpatialGltfModelSource::from_path(path(name)))
}

fn bytes_of(name: &str) -> Vec<u8> {
    std::fs::read(path(name)).expect("a model file")
}

fn near(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-4, "{a:?} is not {b:?}");
}

#[test]
fn a_gltf_with_embedded_buffer_and_texture_gives_a_mesh_with_normals_and_a_bitmap() {
    let model = load("checker_quad.gltf").expect("the quad loads");
    assert_eq!(model.draws.len(), 1);
    let mesh = &model.draws[0].mesh;
    assert_eq!(mesh.positions().len(), 4);
    assert!(
        mesh.normals()
            .iter()
            .all(|&normal| normal == [0.0, 0.0, 1.0])
    );
    // The texture's top left corner is the vertex at the quad's top left
    assert_eq!(mesh.uvs()[0], [0.0, 0.0]);
    assert_eq!(mesh.positions()[0], [-0.5, 0.5, 0.0]);
    assert_eq!(mesh.indices().len(), 6);
    let LayerContent::Bitmap(bitmap) = &model.layers[0].content else {
        panic!("the material has a texture");
    };
    assert_eq!((bitmap.width(), bitmap.height()), (64, 64));
    assert_eq!(model.layers[0].size, [64, 64]);
    // Red in the top left quadrant, yellow in the bottom right
    assert_eq!(bitmap.rgba8_pixel(0), [255, 0, 0, 255]);
    assert_eq!(bitmap.rgba8_pixel(63 * 64 + 63), [255, 255, 0, 255]);
    assert_eq!(model.bounds, ([-0.5, -0.5, 0.0], [0.5, 0.5, 0.0]));
}

#[test]
fn a_glb_keeps_the_node_tree_with_its_transforms() {
    let model = load("tower.glb").expect("the tower loads");
    // The block is the tower's child, and the scene reaches the parent first
    assert_eq!(model.order, [0, 1]);
    assert_eq!(model.parents, [None, Some(0)]);
    assert_eq!(model.rest[1].translation, [2.0, 0.0, 0.0]);
    let (mut locals, mut worlds) = (Vec::new(), Vec::new());
    model.world_transforms([], &mut locals, &mut worlds);
    // The tower's quarter turn about y takes the block's two meters along x to two meters along
    // -z, and the tower lifts everything by one meter
    near(
        worlds[1].transform_point(Vec3::ZERO),
        Vec3::new(0.0, 1.0, -2.0),
    );
    // The box at rest: the block's half sides 0.25, 1 and 0.5 turn x into z
    let (low, high) = model.bounds;
    near(Vec3::from_array(low), Vec3::new(-0.5, 0.0, -2.25));
    near(Vec3::from_array(high), Vec3::new(0.5, 2.0, -1.75));
}

#[test]
fn a_material_with_no_texture_is_one_color_in_srgb() {
    let model = load("tower.glb").expect("the tower loads");
    let LayerContent::Color([red, green, blue, alpha]) = model.layers[0].content else {
        panic!("the material has no texture");
    };
    // The file's factor 0.5, 0.25 and 0.125 is in linear light
    for (channel, expected) in [(red, 0.7354), (green, 0.5370), (blue, 0.3892)] {
        assert!(
            (channel - expected).abs() < 1e-3,
            "{channel} is not {expected}"
        );
    }
    assert_eq!(alpha, 1.0);
}

#[test]
fn a_primitive_with_no_material_and_no_normals_loads_with_the_default_material() {
    let model = load("triangle_external.gltf").expect("the triangle loads");
    assert_eq!(model.draws.len(), 1);
    let mesh = &model.draws[0].mesh;
    // The file has no normals, so the vertices take the triangle's own
    assert!(
        mesh.normals()
            .iter()
            .all(|&normal| normal == [0.0, 0.0, 1.0])
    );
    // The default material is white
    let LayerContent::Color(color) = model.layers[model.draws[0].layer].content else {
        panic!("the default material has no texture");
    };
    assert_eq!(color, [1.0; 4]);
}

#[test]
fn the_buffer_of_a_gltf_is_read_from_the_file_next_to_it() {
    let model = load("triangle_external.gltf").expect("the buffer is next to the file");
    assert_eq!(model.draws[0].mesh.positions().len(), 3);
}

#[test]
fn bytes_have_no_directory_to_find_an_external_buffer_in() {
    let source = SpatialGltfModelSource::from_bytes(bytes_of("triangle_external.gltf"));
    match import(&source) {
        Err(GltfError::MissingBuffer { index: 0, reason }) => {
            assert!(reason.contains("triangle_external.bin"), "{reason}");
        }
        other => panic!("expected a missing buffer, got {other:?}"),
    }
}

#[test]
fn a_buffer_file_that_does_not_exist_fails_with_its_name() {
    match load("missing_buffer.gltf") {
        Err(GltfError::MissingBuffer { index: 0, reason }) => {
            assert!(reason.contains("absent.bin"), "{reason}");
        }
        other => panic!("expected a missing buffer, got {other:?}"),
    }
}

#[test]
fn a_buffer_shorter_than_it_says_fails() {
    let text = r#"{"asset":{"version":"2.0"},
        "buffers":[{"byteLength":100,"uri":"data:application/octet-stream;base64,AAAA"}]}"#;
    match import(&SpatialGltfModelSource::from_bytes(text.as_bytes())) {
        Err(GltfError::MissingBuffer { index: 0, reason }) => {
            assert!(reason.contains("3 bytes"), "{reason}");
        }
        other => panic!("expected a missing buffer, got {other:?}"),
    }
}

#[test]
fn files_that_are_not_models_fail_with_a_reason() {
    let glb = bytes_of("tower.glb");
    let broken: [(&str, Vec<u8>); 5] = [
        ("empty", Vec::new()),
        ("text", b"not a model".to_vec()),
        ("json that is no glTF", br#"{"hello":1}"#.to_vec()),
        // The container's header says more bytes than the file holds
        ("a cut glb", glb[..glb.len() - 64].to_vec()),
        // The json chunk ends in the middle of a string
        ("a glb with a cut json chunk", glb[..60].to_vec()),
    ];
    for (name, bytes) in broken {
        let result = import(&SpatialGltfModelSource::from_bytes(bytes));
        assert!(
            matches!(result, Err(GltfError::Invalid(_))),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn a_file_that_does_not_exist_is_unreadable() {
    let result = load("not_there.glb");
    assert!(
        matches!(&result, Err(GltfError::Unreadable { location, .. }) if location.contains("not_there.glb")),
        "{result:?}"
    );
}

#[test]
fn a_scene_with_nothing_to_draw_is_an_error() {
    let text = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{}]}"#;
    let result = import(&SpatialGltfModelSource::from_bytes(text.as_bytes()));
    assert_eq!(result.err(), Some(GltfError::Empty));
}

#[test]
fn the_animation_of_a_file_moves_its_nodes_by_the_clips_time() {
    let model = load("tower.glb").expect("the tower loads");
    assert_eq!(model.clips.len(), 1);
    assert_eq!(model.clips[0].name.as_deref(), Some("swing"));
    assert_eq!(model.clips[0].duration, 2.0);
    let (mut locals, mut worlds) = (Vec::new(), Vec::new());
    let block = |time: f32, locals: &mut Vec<_>, worlds: &mut Vec<_>| {
        model.world_transforms([(0, time)], locals, worlds);
        worlds[1].transform_point(Vec3::ZERO)
    };
    // Halfway the tower has turned 45 degrees from its start, and the block's step has reached
    // three meters
    let (sin, cos) = std::f32::consts::FRAC_PI_4.sin_cos();
    near(
        block(1.0, &mut locals, &mut worlds),
        Vec3::new(3.0 * sin, 1.0, -3.0 * cos),
    );
    // At the start the tower stands unturned and the block two meters along x
    near(
        block(0.0, &mut locals, &mut worlds),
        Vec3::new(2.0, 1.0, 0.0),
    );
    // Past the end the last keys hold: a quarter turn and four meters
    near(
        block(9.0, &mut locals, &mut worlds),
        Vec3::new(0.0, 1.0, -4.0),
    );
}
