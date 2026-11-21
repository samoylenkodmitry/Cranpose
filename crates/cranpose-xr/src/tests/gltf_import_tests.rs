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

fn text_of(name: &str) -> String {
    std::fs::read_to_string(path(name)).expect("a model file")
}

fn bytes_of(name: &str) -> Vec<u8> {
    std::fs::read(path(name)).expect("a model file")
}

// Whether a normal is +z, the way that the quad and the triangle face.
fn is_up(normal: [f32; 3]) -> bool {
    (Vec3::from_array(normal) - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-6
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
    assert!(mesh.normals().iter().all(|&normal| is_up(normal)));
    // The texture's top left corner is the vertex at the quad's top left
    assert_eq!(mesh.uvs()[0], [0.0, 0.0]);
    assert_eq!(mesh.positions()[0], [-0.5, 0.5, 0.0]);
    assert_eq!(mesh.indices().len(), 6);
    let LayerContent::Bitmap(bitmap) = &model.layers[0].content else {
        panic!("the material has a texture");
    };
    assert_eq!((bitmap.width(), bitmap.height()), (512, 512));
    assert_eq!(model.layers[0].size, [512, 512]);
    // Red in the top left quadrant, yellow in the bottom right
    assert_eq!(bitmap.rgba8_pixel(0), [255, 0, 0, 255]);
    assert_eq!(bitmap.rgba8_pixel(511 * 512 + 511), [255, 255, 0, 255]);
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
    assert!((alpha - 1.0).abs() < 1e-6, "{alpha}");
}

#[test]
fn a_primitive_with_no_material_and_no_normals_loads_with_the_default_material() {
    let model = load("triangle_external.gltf").expect("the triangle loads");
    assert_eq!(model.draws.len(), 1);
    let mesh = &model.draws[0].mesh;
    // The file has no normals, so the vertices take the triangle's own
    assert!(mesh.normals().iter().all(|&normal| is_up(normal)));
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

// The triangle file with its buffer embedded, so that bytes can hold it, and `node` as its node.
fn embedded_triangle(node: &str) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let buffer = STANDARD.encode(bytes_of("triangle_external.bin"));
    text_of("triangle_external.gltf")
        .replace(
            "triangle_external.bin",
            &format!("data:application/octet-stream;base64,{buffer}"),
        )
        .replace(r#""mesh": 0"#, node)
}

#[test]
fn a_node_with_a_matrix_loads_as_its_translation_and_scale() {
    // Twice the size, five meters along x
    let matrix = r#""mesh": 0, "matrix": [2,0,0,0, 0,2,0,0, 0,0,2,0, 5,0,0,1]"#;
    let source = SpatialGltfModelSource::from_bytes(embedded_triangle(matrix).into_bytes());
    let model = import(&source).expect("the triangle loads from bytes");
    assert_eq!(model.rest[0].translation, [5.0, 0.0, 0.0]);
    for scale in model.rest[0].scale {
        assert!((scale - 2.0).abs() < 1e-5, "{scale}");
    }
    // The triangle's corners at 0 and 1 span x from 5 to 7 and y from 0 to 2
    near(Vec3::from_array(model.bounds.0), Vec3::new(5.0, 0.0, 0.0));
    near(Vec3::from_array(model.bounds.1), Vec3::new(7.0, 2.0, 0.0));
}

#[test]
fn a_jpeg_texture_decodes() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut jpeg = Vec::new();
    let red = image::RgbImage::from_pixel(32, 16, image::Rgb([250, 10, 10]));
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&red)
        .expect("a jpeg");
    // The quad's png becomes the jpeg
    let text = text_of("checker_quad.gltf");
    let png_uri = text
        .split('"')
        .find(|part| part.starts_with("data:image/png;base64,"))
        .expect("the png");
    let swapped = text.replace(
        png_uri,
        &format!("data:image/jpeg;base64,{}", STANDARD.encode(&jpeg)),
    );
    let model =
        import(&SpatialGltfModelSource::from_bytes(swapped.into_bytes())).expect("the quad loads");
    let LayerContent::Bitmap(bitmap) = &model.layers[0].content else {
        panic!("the material has a texture");
    };
    assert_eq!((bitmap.width(), bitmap.height()), (32, 16));
    let [red, green, blue, alpha] = bitmap.rgba8_pixel(100);
    assert!(
        red > 200 && green < 60 && blue < 60 && alpha == 255,
        "{red} {green} {blue}"
    );
}

#[test]
fn a_texture_that_names_a_ktx2_image_through_basisu_decodes() {
    let model = load("basisu_quad.gltf").expect("the quad loads");
    let LayerContent::Bitmap(bitmap) = &model.layers[0].content else {
        panic!("the material has a texture");
    };
    assert_eq!((bitmap.width(), bitmap.height()), (4, 4));
    assert_eq!(bitmap.rgba8_pixel(0), [200, 100, 50, 255]);
    assert_eq!(bitmap.rgba8_pixel(15), [200, 100, 50, 255]);
}

#[test]
fn a_basisu_texture_prefers_its_ktx2_image_to_the_source_beside_it() {
    let model = load("basisu_fallback_quad.gltf").expect("the quad loads");
    let LayerContent::Bitmap(bitmap) = &model.layers[0].content else {
        panic!("the material has a texture");
    };
    // The png beside it is 64 pixels wide and has red in its top left corner
    assert_eq!((bitmap.width(), bitmap.height()), (4, 4));
    assert_eq!(bitmap.rgba8_pixel(0), [200, 100, 50, 255]);
}

#[test]
fn a_basisu_texture_that_names_a_missing_image_fails_with_its_index() {
    let text = text_of("basisu_quad.gltf").replace(r#""source": 0"#, r#""source": 7"#);
    let result = parse(text.as_bytes(), None);
    assert!(
        matches!(&result, Err(GltfError::Invalid(reason)) if reason.contains("texture 0")),
        "{result:?}"
    );
}

#[test]
fn a_texture_with_no_source_and_no_extension_fails() {
    let text = text_of("basisu_quad.gltf").replace(
        r#""extensions": {
    "KHR_texture_basisu": {
     "source": 0
    }
   }"#,
        r#""name": "bare""#,
    );
    let result = parse(text.as_bytes(), None);
    assert!(
        matches!(&result, Err(GltfError::Invalid(reason)) if reason.contains("texture 0")),
        "{result:?}"
    );
}

#[test]
fn a_damaged_ktx2_image_fails_as_an_image() {
    let text = text_of("basisu_quad.gltf");
    let uri = text
        .split('"')
        .find(|part| part.starts_with("data:image/ktx2;base64,"))
        .expect("the ktx2 file");
    // The file keeps its identifier and loses the rest
    let mut cut = uri.to_owned();
    cut.truncate(uri.len() - 40);
    let result = parse(text.replace(uri, &cut).as_bytes(), None);
    assert!(matches!(result, Err(GltfError::Image { .. })), "{result:?}");
}

#[test]
fn strips_and_fans_become_triangles_that_keep_their_winding() {
    // A strip of five corners holds three triangles, and every other one turns its first two
    // corners over
    assert_eq!(
        strip_triangles(&[0, 1, 2, 3, 4]),
        [0, 1, 2, 2, 1, 3, 2, 3, 4]
    );
    assert_eq!(fan_triangles(&[9, 1, 2, 3]), [9, 1, 2, 9, 2, 3]);
    assert!(strip_triangles(&[0, 1]).is_empty());
    assert!(fan_triangles(&[]).is_empty());
}
