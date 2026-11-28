// The scene of the volumes with four samples in each pixel, on the GPU of the machine: the edges
// of a mesh show in-between colours where one sample does not, and the depth that the layer
// carries still matches the picture.

use cranpose::space::TextureEdges;

use super::*;

// How far the quad turns about the view axis, so that its edges run at a slant to the pixel grid
const TURN: f32 = 0.5;

// A volume with a red quad that is turned about the volume's z axis, a meter ahead of the eye. The
// mesh's edges are seams, as the edges of a solid's are, so the renderer does not smooth them on
// its own, as it does the outline of a sheet.
fn turned_quad(plane: Arc<Mesh>) -> VolumeComposer {
    Rc::new(move |_, _| {
        let plane = Arc::clone(&plane);
        turned_quad_volume(plane);
    })
}

#[composable]
fn turned_quad_volume(plane: Arc<Mesh>) {
    SpatialVolume(
        SubspaceModifier::new().size(QUAD, QUAD).depth(10.0),
        move |scope| {
            scope.light(Light {
                direction: Vec3::new(0.0, 0.0, 1.0),
                ambient: 1.0,
                shadows: false,
                size: 0.0,
            });
            let plane = Arc::clone(&plane);
            Box(
                Modifier::empty()
                    .required_size(Size::new(CONTENT, CONTENT))
                    .mesh_surface(move || MeshSurface {
                        mesh: Arc::clone(&plane),
                        transform: Mat4::scaling(Vec3::new(QUAD, QUAD, 1.0))
                            .then(&Mat4::translation(Vec3::new(-QUAD / 2.0, -QUAD / 2.0, 0.0)))
                            .then(&Mat4::rotation_z(TURN)),
                        material: Material::Emissive,
                        casts_shadow: false,
                        solid: true,
                    })
                    .background(Color(1.0, 0.0, 0.0, 1.0)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

// What an eye's picture holds: the alpha of each pixel, and the depth beside it.
struct Picture {
    alpha: Vec<u8>,
    depth: Vec<f32>,
}

fn picture(samples: Multisample) -> (Picture, f32) {
    let mut headset = headset_of(turned_quad(Arc::new(
        Mesh::plane().with_texture_edges(TextureEdges::Seams),
    )));
    headset.space.volumes().samples().set(samples);
    let mut pass = VolumePass::default();
    let eye_image = Eye::new(&headset.gpu);
    draw_with_depth(&mut headset, &mut pass, &eye(0.0), &eye_image);
    let near = EyeCamera::of(&eye(0.0), (EYE, EYE), FirstRow::Top)
        .expect("a camera")
        .camera
        .near();
    let alpha = read(&headset.gpu, &eye_image.color, 4)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|texel| texel[3])
        .collect();
    let depth = read_depth(&headset.gpu, &eye_image.depth);
    (Picture { alpha, depth }, 1.0 - near / 1000.0)
}

// The pixels that the quad covers in part: neither empty nor opaque.
fn in_between(picture: &Picture) -> usize {
    picture
        .alpha
        .iter()
        .filter(|alpha| (1..255).contains(*alpha))
        .count()
}

// The rows that an edge of the quad crosses: those that hold both empty and opaque pixels.
fn rows_with_an_edge(picture: &Picture) -> Vec<usize> {
    (0..EYE as usize)
        .filter(|&row| {
            let pixels = &picture.alpha[row * EYE as usize..][..EYE as usize];
            pixels.contains(&0) && pixels.contains(&255)
        })
        .collect()
}

// How many of the rows that an edge crosses hold an in-between pixel, and how many rows an edge
// crosses.
fn rows_with_a_soft_edge(picture: &Picture) -> (usize, usize) {
    let rows = rows_with_an_edge(picture);
    let soft = rows
        .iter()
        .filter(|&&row| {
            picture.alpha[row * EYE as usize..][..EYE as usize]
                .iter()
                .any(|alpha| (1..255).contains(alpha))
        })
        .count();
    (soft, rows.len())
}

#[test]
fn four_samples_smooth_the_edges_that_one_sample_leaves_hard() {
    let (hard, _) = picture(Multisample::Off);
    let (smooth, _) = picture(Multisample::X4);
    let (hard_rows, rows) = rows_with_a_soft_edge(&hard);
    let (smooth_rows, smooth_rows_total) = rows_with_a_soft_edge(&smooth);
    assert!(rows > 20, "the quad's edge crosses only {rows} rows");
    assert_eq!(in_between(&hard), 0, "one sample leaves hard edges");
    assert_eq!(hard_rows, 0);
    assert_eq!(rows, smooth_rows_total);
    // The quad's perimeter is some 150 pixels long, and the pixels that an edge crosses in part
    // take a coverage of a quarter, a half or three quarters
    assert!(
        in_between(&smooth) > 60,
        "four samples gave {} in-between pixels",
        in_between(&smooth)
    );
    assert!(
        smooth_rows * 10 >= rows * 9,
        "{smooth_rows} of {rows} rows that an edge crosses hold an in-between pixel"
    );
}

#[test]
fn the_depth_that_the_layer_carries_matches_the_picture_with_four_samples() {
    let (smooth, quad_depth) = picture(Multisample::X4);
    let pixels = EYE as usize * EYE as usize;
    let (mut opaque, mut empty) = (0, 0);
    for at in 0..pixels {
        match smooth.alpha[at] {
            // A pixel that the quad covers whole carries the quad's depth
            255 => {
                opaque += 1;
                assert!(
                    (smooth.depth[at] - quad_depth).abs() < 1e-4,
                    "an opaque pixel at {at} has depth {}",
                    smooth.depth[at]
                );
            }
            // A pixel with no quad in it carries the far plane
            0 => {
                empty += 1;
                assert!(
                    (smooth.depth[at] - 1.0).abs() < 1e-6,
                    "an empty pixel at {at} has depth {}",
                    smooth.depth[at]
                );
            }
            // A pixel of an edge carries the depth of its centre: the quad's, or the far plane's
            _ => {
                let depth = smooth.depth[at];
                assert!(
                    (depth - quad_depth).abs() < 1e-4 || (depth - 1.0).abs() < 1e-6,
                    "an edge pixel at {at} has depth {depth}"
                );
            }
        }
    }
    assert!(
        opaque > 1000 && empty > 1000,
        "{opaque} opaque, {empty} empty"
    );
}
