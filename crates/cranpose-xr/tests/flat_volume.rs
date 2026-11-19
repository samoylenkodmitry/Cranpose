// A volume in a window, where no headset draws it: the content shows in a box of its width and
// height, seen from the front, with the light that the content set.

use std::sync::Arc;

use cranpose::{
    AppLauncher,
    embedded_view::EmbeddedView,
    prelude::*,
    space::{Light, Mat4, Material, Mesh, MeshSurface, Vec3},
};
use cranpose_xr::{SpatialVolume, Subspace, SubspaceModifier};

const WINDOW: u32 = 400;
// The volume's box is 300 dp wide and high, at the window's top left
const BOX: f32 = 300.0;
// The middle of the box, in pixels
const MIDDLE: u32 = 150;
const RADIUS: f32 = 100.0;

// A window that shows a lit red sphere in a volume. The light travels to the right, so the
// sphere's left side faces it.
fn window() -> EmbeddedView {
    let sphere = Arc::new(Mesh::sphere(48, 24));
    AppLauncher::new()
        .create_embedded_view(WINDOW, WINDOW, 1.0, move || {
            let sphere = Arc::clone(&sphere);
            Subspace(move || {
                let sphere = Arc::clone(&sphere);
                SpatialVolume(
                    SubspaceModifier::new().size(BOX, BOX).depth(BOX),
                    move |scope| {
                        scope.light(Light {
                            direction: Vec3::new(1.0, 0.0, 0.3),
                            ambient: 0.15,
                            shadows: false,
                            size: 0.0,
                        });
                        let sphere = Arc::clone(&sphere);
                        Box(
                            Modifier::empty()
                                .required_size(Size::new(256.0, 128.0))
                                .mesh_surface(move || MeshSurface {
                                    mesh: Arc::clone(&sphere),
                                    transform: Mat4::scaling(Vec3::new(RADIUS, RADIUS, RADIUS)),
                                    material: Material::Lit,
                                    casts_shadow: false,
                                    solid: true,
                                })
                                .background(Color(1.0, 0.0, 0.0, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            });
        })
        .expect("GPU embedded component")
}

// Draws as a host does that honors the view's frame schedule: again while the shell asks for a
// frame.
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for the pipelines that a frame left out, and a poll is the way to see them"
)]
fn draw_settled(view: &mut EmbeddedView, pixels: &mut Vec<u8>) {
    view.draw(pixels).expect("frame");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while view.shell().needs_redraw() {
        assert!(
            std::time::Instant::now() < deadline,
            "the view kept asking for frames"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
        view.draw(pixels).expect("frame");
    }
}

fn red_at(pixels: &[u8], x: u32, y: u32) -> u8 {
    pixels[((y * WINDOW + x) * 4) as usize]
}

#[test]
fn a_volume_shows_its_lit_content_in_a_box_of_its_size() {
    let mut view = window();
    let mut pixels = Vec::new();
    draw_settled(&mut view, &mut pixels);
    // The sphere is red where it faces the light, and darker where it turns away
    let lit = red_at(&pixels, MIDDLE - 60, MIDDLE);
    let shaded = red_at(&pixels, MIDDLE + 60, MIDDLE);
    assert!(lit > 100, "the lit side is {lit}");
    assert!(
        shaded > 0 && lit > shaded + 40,
        "lit {lit}, shaded {shaded}"
    );
    // Outside the sphere the window's background shows, inside the box and beyond it
    assert!(red_at(&pixels, 10, 10) < 40, "the box's corner is empty");
    assert!(red_at(&pixels, 360, 360) < 40, "beyond the box is empty");
}
