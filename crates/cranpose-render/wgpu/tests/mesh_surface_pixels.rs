use std::{
    cell::{Cell, RefCell},
    sync::{Arc, OnceLock},
};

use cranpose_core::{MutableState, remember, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, Color, Modifier, Size,
    space::{
        Camera, EyePose, Glass, Light, Mat4, Material, Mesh, MeshSurface, MeshSurfaces,
        Multisample, NEAR_PLANE, Scene3d, Shaded, TextureEdges, Vec3, Volume,
    },
    widgets::{Box, BoxSpec, Row, RowSpec},
};

use crate::support;

const FRAME_WIDTH: u32 = 400;
const FRAME_HEIGHT: u32 = 300;
const FOCAL_LENGTH: f32 = 600.0;
const EYE_Z: f32 = -800.0;
const SPHERE: Vec3 = Vec3::new(0.0, 0.0, 200.0);
const SPHERE_RADIUS: f32 = 150.0;
const NEAR_PANEL: Vec3 = Vec3::new(60.0, 0.0, 0.0);
const NEAR_PANEL_SIDE: f32 = 100.0;
const SOFT_LIGHT: f32 = 0.1;
const GLOW_PANEL: Vec3 = Vec3::new(-300.0, 250.0, 500.0);
// Off the frame's left edge: the light along (1, 0, 1) throws its shadow
// 600 to the right on the far wall, into the frame.
const NEAR_CASTER: Vec3 = Vec3::new(-500.0, 0.0, 0.0);

fn camera() -> Camera {
    Camera::look_at(
        Vec3::new(0.0, 0.0, EYE_Z),
        Vec3::ZERO,
        Vec3::new(0.0, 1.0, 0.0),
        FOCAL_LENGTH,
    )
}

/// The frame pixel at the scene point `point`.
fn pixel_at(frame: &cranpose_render_wgpu::CapturedFrame, point: Vec3) -> [u8; 4] {
    let shown = camera()
        .project(
            point,
            Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            },
        )
        .expect("in front of the eye");
    let index = (shown.y as usize * frame.width as usize + shown.x as usize) * 4;
    [
        frame.pixels[index],
        frame.pixels[index + 1],
        frame.pixels[index + 2],
        frame.pixels[index + 3],
    ]
}

fn surface(
    mesh: &Arc<Mesh>,
    transform: Mat4,
    (material, casts_shadow): (Material, bool),
) -> Modifier {
    let mesh = Arc::clone(mesh);
    Modifier::empty()
        .required_size(Size {
            width: 256.0,
            height: 128.0,
        })
        .mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&mesh),
            transform,
            material: material.clone(),
            casts_shadow,
            solid: true,
        })
}

#[composable]
fn Surface(
    mesh: Arc<Mesh>,
    transform: Mat4,
    (material, casts_shadow, color): (Material, bool, Color),
) {
    Box(
        surface(&mesh, transform, (material, casts_shadow)).background(color),
        BoxSpec::default(),
        || {},
    );
}

/// A blue wall across the frame behind the scene.
#[composable]
fn FarWall(plane: Arc<Mesh>) {
    Surface(
        plane,
        panel((2000.0, 1500.0), Vec3::new(0.0, 0.0, 600.0)),
        (Material::Unlit, false, Color(0.0, 0.0, 1.0, 1.0)),
    );
}

fn panel(side: (f32, f32), at: Vec3) -> Mat4 {
    Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
        .then(&Mat4::scaling(Vec3::new(side.0, side.1, 1.0)))
        .then(&Mat4::translation(at))
}

fn ball() -> Mat4 {
    Mat4::scaling(Vec3::new(SPHERE_RADIUS, SPHERE_RADIUS, SPHERE_RADIUS))
        .then(&Mat4::translation(SPHERE))
}

fn meshes() -> (Arc<Mesh>, Arc<Mesh>) {
    remember(|| (Arc::new(Mesh::plane()), Arc::new(Mesh::sphere(48, 24))))
        .with(|(plane, sphere)| (Arc::clone(plane), Arc::clone(sphere)))
}

/// A frame-sized scene seen through [`camera`], lit along `(1, 0, 1)`.
#[composable]
fn Scene(light_size: f32, content: impl FnMut() + 'static) {
    SceneOn(Color::BLACK, light_size, content);
}

// `Scene` on a page of `page`, taking the samples `SAMPLES` holds.
#[composable]
fn SceneOn(page: Color, light_size: f32, content: impl FnMut() + 'static) {
    Box(
        Modifier::empty()
            .size(Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            })
            .background(page)
            .scene3d(move || Scene3d {
                camera: camera(),
                light: Some(Light {
                    direction: Vec3::new(1.0, 0.0, 1.0),
                    ambient: 0.2,
                    shadows: true,
                    size: light_size,
                }),
                eye: None,
                multisample: SAMPLES.with(Cell::get),
            }),
        BoxSpec::default(),
        content,
    );
}

#[composable]
fn MeshScene() {
    LitScene(0.0, Material::Lit);
}

#[composable]
fn SoftLightScene() {
    LitScene(SOFT_LIGHT, Material::Lit);
}

#[composable]
fn EmissiveScene() {
    LitScene(0.0, Material::Emissive);
}

// A panel whose shadow falls on the far wall, and a second panel far off
// to the side whose shadow falls nowhere the wall is.
#[composable]
fn FarCasterScene() {
    let (plane, _) = meshes();
    Scene(0.0, move || {
        for at in [NEAR_CASTER, Vec3::new(40_000.0, 0.0, 0.0)] {
            Surface(
                Arc::clone(&plane),
                panel((200.0, 200.0), at),
                (Material::Unlit, true, Color(0.0, 1.0, 0.0, 1.0)),
            );
        }
        FarWall(Arc::clone(&plane));
    });
}

#[composable]
fn LitScene(light_size: f32, sphere_material: Material) {
    let (plane, sphere) = meshes();
    Scene(light_size, move || {
        // The near panel comes first: depth, not order, hides the sphere.
        Surface(
            Arc::clone(&plane),
            panel((NEAR_PANEL_SIDE, NEAR_PANEL_SIDE), NEAR_PANEL),
            (Material::Unlit, false, Color(0.0, 1.0, 0.0, 1.0)),
        );
        Surface(
            Arc::clone(&sphere),
            ball(),
            (sphere_material.clone(), true, Color(1.0, 0.0, 0.0, 1.0)),
        );
        FarWall(Arc::clone(&plane));
    });
}

/// A clear glass ball in front of a wall whose left half is red and right
/// half blue, the line between them behind the ball's middle; a glow panel
/// over the wall's red half, below the ball.
#[composable]
fn GlassScene() {
    let (plane, sphere) = meshes();
    Scene(0.0, move || {
        Surface(
            Arc::clone(&plane),
            panel((300.0, 60.0), GLOW_PANEL),
            (Material::Glow, false, Color(0.0, 0.5, 0.0, 1.0)),
        );
        Surface(
            Arc::clone(&sphere),
            ball(),
            (
                Material::Glass(Glass::clear(2.0 * SPHERE_RADIUS)),
                false,
                Color::TRANSPARENT,
            ),
        );
        red_blue_wall(&plane, panel((2000.0, 1500.0), Vec3::new(0.0, 0.0, 600.0)));
    });
}

/// A wall placed by `transform`, its left half red and its right half blue.
fn red_blue_wall(plane: &Arc<Mesh>, transform: Mat4) {
    Row(
        surface(plane, transform, (Material::Unlit, false)),
        RowSpec::default(),
        red_blue_halves,
    );
}

// A red square on the left, a blue one on the right.
fn red_blue_halves() {
    halves(RED);
}

// A square of `left` on the left, a blue one on the right.
fn halves(left: Color) {
    for color in [left, Color(0.0, 0.0, 1.0, 1.0)] {
        Box(
            Modifier::empty()
                .size(Size {
                    width: 128.0,
                    height: 128.0,
                })
                .background(color),
            BoxSpec::default(),
            || {},
        );
    }
}

// The wall of `HeldWallScene`.
const HELD_WALL: Vec3 = Vec3::new(0.0, 0.0, 300.0);
const HELD_WALL_SIDE: (f32, f32) = (400.0, 200.0);

// A red and blue wall whose node a parent narrower than its content holds:
// it lays out its content at its own size, past the parent's width.
#[composable]
fn HeldWallScene() {
    let (plane, _) = meshes();
    Scene(0.0, move || {
        let plane = Arc::clone(&plane);
        Box(
            Modifier::empty().size(Size {
                width: 64.0,
                height: 64.0,
            }),
            BoxSpec::default(),
            move || {
                let plane = Arc::clone(&plane);
                Row(
                    Modifier::empty()
                        .wrap_content_size(Alignment::TOP_START, true)
                        .size(Size {
                            width: 256.0,
                            height: 128.0,
                        })
                        .mesh_surface(move || MeshSurface {
                            mesh: Arc::clone(&plane),
                            transform: panel(HELD_WALL_SIDE, HELD_WALL),
                            material: Material::Unlit,
                            casts_shadow: false,
                            solid: true,
                        }),
                    RowSpec::default(),
                    red_blue_halves,
                );
            },
        );
    });
}

// Where `TwoMeshScene` shows its one content: the owner and a smaller copy.
const OWNER_WALL: (Vec3, (f32, f32)) = (Vec3::new(-90.0, -20.0, 0.0), (160.0, 80.0));
const COPY_WALL: (Vec3, (f32, f32)) = (Vec3::new(100.0, 50.0, 0.0), (120.0, 60.0));

const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);

// One content on two meshes, its left half the colour the test sets.
#[composable]
fn TwoMeshScene() {
    let (plane, _) = meshes();
    let left = rememberMutableStateOf(|| RED);
    TWO_MESH_LEFT.with(|slot| *slot.borrow_mut() = Some(left));
    Scene(0.0, move || {
        let plane = Arc::clone(&plane);
        Row(
            Modifier::empty()
                .size(Size {
                    width: 256.0,
                    height: 128.0,
                })
                .mesh_surfaces(move || {
                    let placed = |(at, side)| MeshSurface {
                        mesh: Arc::clone(&plane),
                        transform: panel(side, at),
                        material: Material::Unlit,
                        casts_shadow: false,
                        solid: true,
                    };
                    MeshSurfaces::new(placed(OWNER_WALL), vec![placed(COPY_WALL)])
                }),
            RowSpec::default(),
            move || halves(left.get()),
        );
    });
}

#[test]
fn a_layer_on_two_meshes_shows_its_content_on_both() {
    let Some((_lock, mut shell)) = support::app_shell_for(
        TwoMeshScene,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    ) else {
        return;
    };
    let (frame, _) = support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    both_show_left(&frame, ("red", 0));
    let left = TWO_MESH_LEFT.with(|slot| slot.borrow().expect("the scene composed"));
    left.set(GREEN);
    let (_, frame) = support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    both_show_left(&frame, ("green", 1));
}

// Asserts that both meshes of `TwoMeshScene` show the content's left half
// in the colour `name`, whose `channel` is full, left of its blue half.
fn both_show_left(frame: &cranpose_render_wgpu::CapturedFrame, (name, channel): (&str, usize)) {
    for (at, (width, _)) in [OWNER_WALL, COPY_WALL] {
        let left = pixel_at(frame, at + Vec3::new(-width * 0.25, 0.0, 0.0));
        let right = pixel_at(frame, at + Vec3::new(width * 0.25, 0.0, 0.0));
        assert!(
            left[channel] > 200 && left[1 - channel] < 40 && left[2] < 40 && right[2] > 200,
            "the mesh at {at:?} shows the content's {name} half left of its blue one: \
             {left:?}, {right:?}"
        );
    }
}

#[test]
fn mesh_surfaces_draw_their_content_lit_shadowed_and_nearest_first() {
    let Some((_lock, mut shell)) = support::app_shell_for(
        MeshScene,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    ) else {
        return;
    };
    let (frame, _) = support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);

    let near_panel = pixel_at(&frame, NEAR_PANEL);
    assert!(
        near_panel[1] > 200 && near_panel[0] < 40,
        "the near panel hides the sphere behind it: {near_panel:?}"
    );

    let lit_side = pixel_at(&frame, SPHERE + Vec3::new(-0.6, 0.0, -0.8) * SPHERE_RADIUS);
    let dark_side = pixel_at(
        &frame,
        SPHERE + Vec3::new(-0.2, 0.75, -0.63) * SPHERE_RADIUS,
    );
    assert!(
        lit_side[0] > 150 && lit_side[1] < 40 && lit_side[2] < 40,
        "the sphere shows its red content where the light falls: {lit_side:?}"
    );
    assert!(
        dark_side[0] + 40 < lit_side[0],
        "the sphere is shaded: {dark_side:?} away from the light, {lit_side:?} facing it"
    );

    let far_wall = pixel_at(&frame, Vec3::new(-300.0, 0.0, 600.0));
    assert!(
        far_wall[2] > 200 && far_wall[0] < 40,
        "the far wall shows beside the sphere, in the light: {far_wall:?}"
    );
    // The light travels along (1, 0, 1): the sphere's shadow falls on the
    // far wall 400 to the right of its centre.
    let shadow = pixel_at(&frame, SPHERE + Vec3::new(400.0, 0.0, 400.0));
    assert!(
        shadow[2] < 100 && shadow[2] > 20,
        "the sphere's shadow darkens the unlit far wall to its ambient: {shadow:?}"
    );
}

/// How many pixels along the column through the middle of the sphere's
/// shadow on the far wall, from the middle up past its top edge, lie between
/// shadow and light.
fn shadow_edge_width(frame: &cranpose_render_wgpu::CapturedFrame) -> usize {
    let middle = camera()
        .project(
            SPHERE + Vec3::new(400.0, 0.0, 400.0),
            Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            },
        )
        .expect("in front of the eye");
    let (x, y) = (middle.x as usize, middle.y as usize);
    (0..y)
        .filter(|&row| {
            let blue = frame.pixels[(row * frame.width as usize + x) * 4 + 2];
            (70..230).contains(&blue)
        })
        .count()
}

fn capture(page: fn()) -> Option<cranpose_render_wgpu::CapturedFrame> {
    capture_with(page, Multisample::Off)
}

// A frame of `page`, whose `Scene` takes `samples` in each pixel.
fn capture_with(page: fn(), samples: Multisample) -> Option<cranpose_render_wgpu::CapturedFrame> {
    SAMPLES.with(|cell| cell.set(samples));
    let frame = support::app_shell_for(
        page,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    )
    .map(|(_lock, mut shell)| support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT).0);
    SAMPLES.with(|cell| cell.set(Multisample::Off));
    frame
}

thread_local! {
    // The samples a `Scene` the test draws takes in each pixel.
    static SAMPLES: Cell<Multisample> = const { Cell::new(Multisample::Off) };
}

/// A side of content whose square at scale one is past the 16M-pixel budget
/// of a layer's surface.
const PAST_THE_BUDGET: f32 = 4200.0;

/// A green wall across the frame whose content is larger than a surface may
/// be at the frame's scale.
#[composable]
fn LargeWallScene() {
    let (plane, _) = meshes();
    Scene(0.0, move || {
        let plane = Arc::clone(&plane);
        Box(
            Modifier::empty()
                .required_size(Size {
                    width: PAST_THE_BUDGET,
                    height: PAST_THE_BUDGET,
                })
                .mesh_surface(move || MeshSurface {
                    mesh: Arc::clone(&plane),
                    transform: panel((2000.0, 1500.0), Vec3::new(0.0, 0.0, 600.0)),
                    material: Material::Unlit,
                    casts_shadow: false,
                    solid: true,
                })
                .background(Color(0.0, 1.0, 0.0, 1.0)),
            BoxSpec::default(),
            || {},
        );
    });
}

#[test]
fn a_mesh_surface_past_the_surface_budget_draws_its_content_at_a_lower_scale() {
    let Some(frame) = capture(LargeWallScene) else {
        return;
    };
    let wall = pixel_at(&frame, Vec3::new(0.0, 0.0, 600.0));
    assert!(
        wall[1] > 240 && wall[0] < 16 && wall[2] < 16,
        "the wall shows its green content: {wall:?}"
    );
}

#[test]
fn a_surface_its_parent_holds_narrower_shows_all_of_its_content() {
    let Some(frame) = capture(HeldWallScene) else {
        return;
    };
    let quarter = Vec3::new(HELD_WALL_SIDE.0 * 0.25, 0.0, 0.0);
    let left = pixel_at(&frame, HELD_WALL - quarter);
    let right = pixel_at(&frame, HELD_WALL + quarter);
    assert!(
        left[0] > 240 && left[2] < 16,
        "the wall's left half shows its red content: {left:?}"
    );
    assert!(
        right[2] > 240 && right[0] < 16,
        "the wall's right half shows its blue content, past the parent's width: {right:?}"
    );
}

// A blue wall, turned so its left edge comes toward the eye, and on it,
// one unit nearer the eye, a translucent red panel inside its outline.
const DECAL_WALL: Vec3 = Vec3::new(0.0, 0.0, 300.0);

fn turned_panel(side: (f32, f32), at: Vec3) -> Mat4 {
    Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
        .then(&Mat4::scaling(Vec3::new(side.0, side.1, 1.0)))
        .then(&Mat4::rotation_y(0.5))
        .then(&Mat4::translation(at))
}

#[composable]
fn DecalScene() {
    let (plane, _) = meshes();
    Scene(0.0, move || {
        Surface(
            Arc::clone(&plane),
            turned_panel((400.0, 300.0), DECAL_WALL),
            (Material::Unlit, false, Color(0.0, 0.0, 1.0, 1.0)),
        );
        Surface(
            Arc::clone(&plane),
            turned_panel((320.0, 220.0), DECAL_WALL - Vec3::new(0.0, 0.0, 1.0)),
            (Material::Unlit, false, Color(1.0, 0.0, 0.0, 0.5)),
        );
    });
}

#[test]
fn a_translucent_surface_on_another_shows_it_through_from_any_side() {
    let Some(frame) = capture(DecalScene) else {
        return;
    };
    let middle = pixel_at(&frame, DECAL_WALL - Vec3::new(0.0, 0.0, 1.0));
    assert!(
        middle[0] > 100 && middle[2] > 100,
        "the red panel shows the blue wall through it, not the black behind the wall: {middle:?}"
    );
}

#[test]
fn a_light_of_some_size_softens_the_edge_of_a_shadow() {
    let Some(hard) = capture(MeshScene) else {
        return;
    };
    let Some(soft) = capture(SoftLightScene) else {
        return;
    };
    let (hard, soft) = (shadow_edge_width(&hard), shadow_edge_width(&soft));
    assert!(
        hard <= 6,
        "a light of no size casts a hard edge: {hard} pixels"
    );
    assert!(
        soft >= 2 * hard.max(4),
        "a light of some size widens the edge: {soft} pixels against {hard}"
    );
}

#[test]
fn a_caster_far_from_every_receiver_leaves_shadow_edges_sharp() {
    let Some(frame) = capture(FarCasterScene) else {
        return;
    };
    // The near panel's shadow ends 100 to the right of x = 600 - 500, on the
    // wall 1400 from the eye: one pixel there is 1400 / FOCAL_LENGTH wide.
    let pixel = (600.0 - EYE_Z) / FOCAL_LENGTH;
    let blurred = std::iter::successors(Some(200.0 - 30.0 * pixel), |x| Some(x + pixel))
        .take(60)
        .filter(|&x| (70..230).contains(&pixel_at(&frame, Vec3::new(x, 0.0, 600.0))[2]))
        .count();
    assert!(
        blurred <= 4,
        "the shadow's edge stays sharp with a caster far away: {blurred} pixels wide"
    );
}

#[test]
fn an_emissive_surface_shows_its_colour_untouched_by_light() {
    let Some(frame) = capture(EmissiveScene) else {
        return;
    };
    let facing = pixel_at(&frame, SPHERE + Vec3::new(-0.6, 0.0, -0.8) * SPHERE_RADIUS);
    let away = pixel_at(
        &frame,
        SPHERE + Vec3::new(-0.2, 0.75, -0.63) * SPHERE_RADIUS,
    );
    assert!(
        facing[0] > 240 && away[0] > 240,
        "the emissive sphere is red at full strength on both sides: {facing:?} and {away:?}"
    );
}

// An emissive red sphere, which no light changes, on a grey page.
#[composable]
fn OutlineScene() {
    let (_, sphere) = meshes();
    SceneOn(Color(0.5, 0.5, 0.5, 1.0), 0.0, move || {
        Surface(
            Arc::clone(&sphere),
            ball(),
            (Material::Emissive, false, Color(1.0, 0.0, 0.0, 1.0)),
        );
    });
}

// The pixels of a frame of `OutlineScene` that are neither the page's grey
// nor the sphere's red, and how many of those are not a blend of the two on
// the sphere's outline.
fn outline_blends(frame: &cranpose_render_wgpu::CapturedFrame) -> (usize, usize) {
    let page = [frame.pixels[0], frame.pixels[1], frame.pixels[2]];
    // The sphere lies on the camera's axis, so its outline is a circle about
    // the frame's middle.
    let distance = SPHERE.z - EYE_Z;
    let outline =
        FOCAL_LENGTH * SPHERE_RADIUS / (distance * distance - SPHERE_RADIUS * SPHERE_RADIUS).sqrt();
    let near = |a: u8, b: u8| a.abs_diff(b) <= 2;
    let (mut blends, mut strays) = (0, 0);
    for (index, pixel) in frame.pixels.as_chunks::<4>().0.iter().enumerate() {
        let is_page = (0..3).all(|channel| near(pixel[channel], page[channel]));
        let is_red = pixel[0] == 255 && pixel[1] == 0 && pixel[2] == 0;
        if is_page || is_red {
            continue;
        }
        blends += 1;
        let x = (index % frame.width as usize) as f32 + 0.5 - FRAME_WIDTH as f32 * 0.5;
        let y = (index / frame.width as usize) as f32 + 0.5 - FRAME_HEIGHT as f32 * 0.5;
        let on_outline = ((x * x + y * y).sqrt() - outline).abs() < 1.5;
        let red_over_grey = near(pixel[1], pixel[2]) && pixel[0] > pixel[1];
        if !(on_outline && red_over_grey) {
            strays += 1;
        }
    }
    (blends, strays)
}

#[test]
fn a_sphere_s_outline_blends_into_the_page_with_four_samples_and_steps_with_one() {
    let Some(single) = capture_with(OutlineScene, Multisample::Off) else {
        return;
    };
    let Some(four) = capture_with(OutlineScene, Multisample::X4) else {
        return;
    };
    assert_eq!(
        outline_blends(&single),
        (0, 0),
        "with one sample each pixel shows the sphere or the page"
    );
    let (blends, strays) = outline_blends(&four);
    assert!(
        blends > 300,
        "with four samples the pixels the outline crosses blend the sphere into the page: {blends}"
    );
    assert_eq!(
        strays, 0,
        "every blended pixel lies on the outline, red over grey"
    );
    for frame in [&single, &four] {
        assert_eq!(
            &frame.pixels[..4],
            &[128, 128, 128, 255],
            "the page keeps its grey away from the sphere"
        );
    }
}

// The red sphere of `OutlineScene` in a grey scene of `size` whose node lies
// at `at` in the frame, on a white page.
#[composable]
fn PlacedOutline(at: (f32, f32), size: Size) {
    let (_, sphere) = meshes();
    Box(
        Modifier::empty()
            .size(Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            })
            .background(Color::WHITE),
        BoxSpec::default(),
        move || {
            let sphere = Arc::clone(&sphere);
            Box(
                Modifier::empty()
                    .offset(at.0, at.1)
                    .size(size)
                    .background(Color(0.5, 0.5, 0.5, 1.0))
                    .scene3d(|| Scene3d {
                        camera: camera(),
                        light: None,
                        eye: None,
                        multisample: SAMPLES.with(Cell::get),
                    }),
                BoxSpec::default(),
                move || {
                    Surface(
                        Arc::clone(&sphere),
                        ball(),
                        (Material::Emissive, false, Color(1.0, 0.0, 0.0, 1.0)),
                    );
                },
            );
        },
    );
}

// A scene smaller than the frame, inside it.
#[composable]
fn SmallOutlineScene() {
    PlacedOutline((130.0, 100.0), Size::new(200.0, 150.0));
}

// A scene of the frame's size that hangs past its right and bottom edges.
#[composable]
fn HangingOutlineScene() {
    PlacedOutline(
        (60.0, 45.0),
        Size::new(FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
    );
}

// How many pixels of `four` differ from `one`, and how many of those lie
// where `one` shows one colour over the pixel and its eight neighbours.
fn sampled_changes(
    one: &cranpose_render_wgpu::CapturedFrame,
    four: &cranpose_render_wgpu::CapturedFrame,
) -> (usize, usize) {
    let width = one.width as usize;
    let height = one.height as usize;
    let at = |frame: &cranpose_render_wgpu::CapturedFrame, x: usize, y: usize| {
        let index = (y * width + x) * 4;
        [
            frame.pixels[index],
            frame.pixels[index + 1],
            frame.pixels[index + 2],
        ]
    };
    let (mut changed, mut misplaced) = (0, 0);
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            if at(one, x, y) == at(four, x, y) {
                continue;
            }
            changed += 1;
            let flat = (y - 1..=y + 1)
                .all(|row| (x - 1..=x + 1).all(|column| at(one, column, row) == at(one, x, y)));
            if flat {
                misplaced += 1;
            }
        }
    }
    (changed, misplaced)
}

#[test]
fn a_multisampled_scene_lands_where_it_lies_on_the_page() {
    for scene in [SmallOutlineScene as fn(), HangingOutlineScene] {
        let (Some(one), Some(four)) = (
            capture_with(scene, Multisample::Off),
            capture_with(scene, Multisample::X4),
        ) else {
            return;
        };
        let (changed, misplaced) = sampled_changes(&one, &four);
        assert!(
            changed > 100,
            "four samples blend the sphere's outline: {changed}"
        );
        assert_eq!(
            misplaced, 0,
            "four samples change only pixels an edge crosses"
        );
    }
}

#[test]
fn a_glass_ball_shows_the_scene_behind_it_bent_and_glow_adds_to_it() {
    // Each pipeline the scene draws with has a variant of four samples.
    for samples in [Multisample::Off, Multisample::X4] {
        let Some(frame) = capture_with(GlassScene, samples) else {
            return;
        };
        let left_of_ball = pixel_at(&frame, Vec3::new(-300.0, 0.0, 600.0));
        assert!(
            left_of_ball[0] > 200 && left_of_ball[2] < 40,
            "the wall's left half is red beside the ball: {left_of_ball:?}"
        );
        // Through a solid ball the scene shows upside down: the ball's left side
        // shows the wall's right half, and its right side the left half.
        let ball_left = pixel_at(
            &frame,
            SPHERE + Vec3::new(-0.5, 0.0, -0.866) * SPHERE_RADIUS,
        );
        let ball_right = pixel_at(&frame, SPHERE + Vec3::new(0.5, 0.0, -0.866) * SPHERE_RADIUS);
        assert!(
            ball_left[2] > ball_left[0] + 60,
            "the ball's left side shows the blue right half of the wall: {ball_left:?}"
        );
        assert!(
            ball_right[0] > ball_right[2] + 60,
            "the ball's right side shows the red left half of the wall: {ball_right:?}"
        );
        let glow = pixel_at(&frame, GLOW_PANEL);
        assert!(
            glow[0] > 200 && glow[1] > 100,
            "the glow panel adds its green to the red wall it hides nothing of: {glow:?}"
        );
    }
}

thread_local! {
    static SHAPE: RefCell<Option<MutableState<Arc<Mesh>>>> = const { RefCell::new(None) };
    static WALKER: RefCell<Option<cranpose_ui::Eye>> = const { RefCell::new(None) };
    static TWO_MESH_LEFT: RefCell<Option<MutableState<Color>>> = const { RefCell::new(None) };
}

/// A sphere of `radius` from its signed distance field.
fn sdf_ball(radius: f32) -> Arc<Mesh> {
    let reach = radius * 1.25;
    Arc::new(
        Mesh::from_sdf(
            move |point| point.length() - radius,
            (
                Vec3::new(-reach, -reach, -reach),
                Vec3::new(reach, reach, reach),
            ),
            24,
        )
        .expect("the ball's surface lies in its box"),
    )
}

/// A ball whose mesh the test replaces, in front of a blue wall.
#[composable]
fn ChangingScene() {
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    let shape = rememberMutableStateOf(|| sdf_ball(SPHERE_RADIUS));
    SHAPE.with(|slot| *slot.borrow_mut() = Some(shape));
    Scene(0.0, move || {
        Surface(
            shape.get(),
            Mat4::translation(SPHERE),
            (Material::Emissive, false, Color(1.0, 0.0, 0.0, 1.0)),
        );
        FarWall(Arc::clone(&plane));
    });
}

#[test]
fn a_mesh_replaced_by_a_smaller_one_draws_the_new_shape() {
    let Some((_lock, mut shell)) = support::app_shell_for(
        ChangingScene,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    ) else {
        return;
    };
    let (frame, _) = support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let near_rim = SPHERE + Vec3::new(-0.8, 0.0, -0.6) * SPHERE_RADIUS;
    assert!(
        pixel_at(&frame, near_rim)[0] > 200,
        "the large ball covers its rim"
    );

    for radius in [SPHERE_RADIUS * 0.4, SPHERE_RADIUS * 0.5] {
        let shape = SHAPE.with(|slot| slot.borrow().expect("the scene composed"));
        shape.set(sdf_ball(radius));
        let (_, frame) = support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
        let rim = pixel_at(&frame, near_rim);
        assert!(
            rim[2] > 200 && rim[0] < 40,
            "the wall shows where the large ball was, beyond the ball of radius {radius}: {rim:?}"
        );
        let middle = pixel_at(&frame, SPHERE + Vec3::new(0.0, 0.0, -radius));
        assert!(middle[0] > 200, "the smaller ball shows: {middle:?}");
    }
}

/// Red: the first value. Green: how squarely the surface faces the eye.
/// Blue: how much of the light reaches it.
const FACING_WGSL: &str = r"
fn shade(surface: ShadedSurface) -> vec4<f32> {
    let facing = dot(surface.normal, surface.to_eye);
    return vec4<f32>(shaded_param(0u), facing, surface.lit, 1.0) * surface.color.a;
}
";

fn shaded_of(source: &'static OnceLock<Arc<str>>, text: &str) -> Material {
    let mut params = [0.0; Shaded::PARAMS];
    params[0] = 1.0;
    Material::Shaded(Shaded {
        shader: Arc::clone(source.get_or_init(|| Arc::from(text))),
        params,
    })
}

/// A lit red ball in front of a far wall with blue content that `wall`
/// colours, in a light along `(1, 0, 1)` that casts the ball's shadow on
/// the wall.
#[composable]
fn ShadedScene(wall: Material) {
    let (plane, sphere) = meshes();
    Scene(0.0, move || {
        Surface(
            Arc::clone(&sphere),
            ball(),
            (Material::Lit, true, Color(1.0, 0.0, 0.0, 1.0)),
        );
        Surface(
            Arc::clone(&plane),
            panel((2000.0, 1500.0), Vec3::new(0.0, 0.0, 600.0)),
            (wall.clone(), false, Color(0.0, 0.0, 1.0, 1.0)),
        );
    });
}

#[composable]
fn FacingScene() {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    ShadedScene(shaded_of(&SHADER, FACING_WGSL));
}

#[composable]
fn BrokenShadedScene() {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    ShadedScene(shaded_of(&SHADER, "fn shade() {}"));
}

#[test]
fn a_shaded_surface_takes_its_colour_from_its_shader() {
    // Each pipeline the scene draws with has a variant of four samples.
    for samples in [Multisample::Off, Multisample::X4] {
        let Some(frame) = capture_with(FacingScene, samples) else {
            return;
        };
        let lit = pixel_at(&frame, Vec3::new(-400.0, 300.0, 600.0));
        assert!(
            lit[0] > 240 && lit[1] > 200 && lit[2] > 240,
            "the wall shows its first value, that it faces the eye and that the light reaches it: {lit:?}"
        );
        let shadowed = pixel_at(&frame, Vec3::new(400.0, 0.0, 600.0));
        assert!(
            shadowed[0] > 240 && shadowed[2] < 40,
            "in the ball's shadow the light does not reach the wall: {shadowed:?}"
        );
    }
}

// Green where the eye the surface holds lies within a unit of the point
// its values 1 to 3 give, red elsewhere.
const EYE_WGSL: &str = r"
fn shade(surface: ShadedSurface) -> vec4<f32> {
    let expected = vec3<f32>(shaded_param(1u), shaded_param(2u), shaded_param(3u));
    let near = length(surface.eye - expected) < 1.0;
    return select(vec4<f32>(1.0, 0.0, 0.0, 1.0), vec4<f32>(0.0, 1.0, 0.0, 1.0), near);
}
";

// The wall of `ShadedScene`, whose shader expects the camera's eye in the
// wall's mesh.
#[composable]
fn EyeScene() {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let wall = panel((2000.0, 1500.0), Vec3::new(0.0, 0.0, 600.0));
    let eye = wall
        .inverse()
        .expect("the wall spans space")
        .transform_point(camera().eye());
    let mut params = [0.0; Shaded::PARAMS];
    params[1..4].copy_from_slice(&eye.to_array());
    ShadedScene(Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(EYE_WGSL))),
        params,
    }));
}

#[test]
fn a_shaded_surface_knows_where_the_eye_stands_in_its_mesh() {
    let Some(frame) = capture(EyeScene) else {
        return;
    };
    let wall = pixel_at(&frame, Vec3::new(-400.0, 300.0, 600.0));
    assert!(
        wall[1] > 200 && wall[0] < 40,
        "the wall's shader reads the camera's eye in the wall's mesh: {wall:?}"
    );
}

#[test]
fn a_shaded_surface_whose_shader_does_not_build_draws_lit() {
    let Some(frame) = capture(BrokenShadedScene) else {
        return;
    };
    let lit = pixel_at(&frame, Vec3::new(-400.0, 300.0, 600.0));
    assert!(
        lit[2] > 150 && lit[0] < 40 && lit[1] < 40,
        "the wall shows its blue content, lit: {lit:?}"
    );
}

/// A ball of radius 0.4 about the middle of the unit cube, in its content's
/// colour times the first value.
const BALL_VOLUME_WGSL: &str = r"
fn volume(eye: vec3<f32>, direction: vec3<f32>, entry: f32, exit: f32, footprint: f32) -> VolumeHit {
    var hit: VolumeHit;
    hit.color = vec4<f32>(0.0);
    hit.at = eye;
    let b = dot(eye, direction);
    let h = b * b - dot(eye, eye) + 0.16;
    if (h < 0.0) {
        return hit;
    }
    let t = -b - sqrt(h);
    if (t < entry || t > exit) {
        return hit;
    }
    hit.at = eye + direction * t;
    hit.color = vec4<f32>(volume_content(vec2<f32>(0.5)).rgb * volume_param(0u), 1.0);
    return hit;
}
";

/// The side of the cube the ball volume fills, about [`SPHERE`].
const VOLUME_SIDE: f32 = 300.0;

fn volume_of(source: &'static OnceLock<Arc<str>>, text: &str) -> Material {
    let mut params = [0.0; Volume::PARAMS];
    params[0] = 1.0;
    Material::Volume(Volume {
        shader: Arc::clone(source.get_or_init(|| Arc::from(text))),
        params,
    })
}

/// A ball drawn by a volume in a cube, with a green panel in front of the
/// ball within the cube and one behind the ball's front within the cube, in
/// front of a blue wall.
#[composable]
fn VolumeScene(volume: Material) {
    let (plane, cube) = remember(|| (Arc::new(Mesh::plane()), Arc::new(Mesh::cuboid())))
        .with(|(plane, cube)| (Arc::clone(plane), Arc::clone(cube)));
    Scene(0.0, move || {
        for (at, side) in [
            (Vec3::new(80.0, 0.0, 80.0), 40.0),
            (Vec3::new(-50.0, 0.0, 170.0), 30.0),
        ] {
            Surface(
                Arc::clone(&plane),
                panel((side, side), at),
                (Material::Unlit, false, Color(0.0, 1.0, 0.0, 1.0)),
            );
        }
        Surface(
            Arc::clone(&cube),
            Mat4::scaling(Vec3::new(VOLUME_SIDE, VOLUME_SIDE, VOLUME_SIDE))
                .then(&Mat4::translation(SPHERE)),
            (volume.clone(), true, Color(1.0, 0.0, 0.0, 1.0)),
        );
        FarWall(Arc::clone(&plane));
    });
}

#[composable]
fn BallVolumeScene() {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    VolumeScene(volume_of(&SHADER, BALL_VOLUME_WGSL));
}

#[composable]
fn BrokenVolumeScene() {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    VolumeScene(volume_of(&SHADER, "fn volume() {}"));
}

#[test]
fn a_volume_draws_what_its_shader_finds_at_the_depth_it_finds_it() {
    // Each pipeline the scene draws with has a variant of four samples.
    for samples in [Multisample::Off, Multisample::X4] {
        let Some(frame) = capture_with(BallVolumeScene, samples) else {
            return;
        };
        let radius = 0.4 * VOLUME_SIDE;
        let front = pixel_at(&frame, SPHERE - Vec3::new(0.0, 0.0, radius));
        assert!(
            front[0] > 200 && front[1] < 40 && front[2] < 40,
            "the ball shows its content's red, times its first value: {front:?}"
        );
        let corner = pixel_at(&frame, SPHERE + Vec3::new(130.0, 130.0, -150.0));
        assert!(
            corner[2] > 200 && corner[0] < 40,
            "the wall shows through the cube where the ball is not: {corner:?}"
        );
        let ahead = pixel_at(&frame, Vec3::new(80.0, 0.0, 80.0));
        assert!(
            ahead[1] > 200 && ahead[0] < 40,
            "a panel in the cube in front of the ball hides it: {ahead:?}"
        );
        let behind = pixel_at(&frame, Vec3::new(-50.0, 0.0, 170.0));
        assert!(
            behind[0] > 200 && behind[1] < 40,
            "the ball hides a panel behind its front: {behind:?}"
        );
        // The light travels along (1, 0, 1): the cube would shade this point of
        // the wall, beside the ball's shadow.
        let unshaded = pixel_at(&frame, Vec3::new(300.0, 130.0, 600.0));
        assert!(
            unshaded[2] > 200,
            "a volume casts no shadow, though its surface asks for one: {unshaded:?}"
        );
    }
}

#[test]
fn a_volume_whose_shader_does_not_build_is_left_out() {
    let Some(frame) = capture(BrokenVolumeScene) else {
        return;
    };
    let front = pixel_at(&frame, SPHERE - Vec3::new(0.0, 0.0, 0.4 * VOLUME_SIDE));
    assert!(
        front[2] > 200 && front[0] < 40,
        "the wall shows where the ball would be: {front:?}"
    );
}

/// A blue wall far away, 1024 by 512 scene pixels at a depth of 600 with
/// content of 256 by 128, whose content holds what the node holds.
fn blue_wall(plane: &Arc<Mesh>) -> Modifier {
    surface(
        plane,
        panel((1024.0, 512.0), Vec3::new(0.0, 0.0, 600.0)),
        (Material::Unlit, false),
    )
    .background(Color(0.0, 0.0, 1.0, 1.0))
}

/// A blue wall far away, a green ball in front of it, and a red panel inside
/// the wall's content that stands out of the wall in front of the ball. The
/// wall's 256 content pixels span 1024 scene pixels, so a content pixel of
/// the wall is four scene pixels in each direction, depth included.
#[composable]
fn JoinedScene() {
    let (plane, sphere) = meshes();
    Scene(0.0, move || {
        Surface(
            Arc::clone(&sphere),
            Mat4::scaling(Vec3::new(100.0, 100.0, 100.0))
                .then(&Mat4::translation(Vec3::new(0.0, 0.0, 400.0))),
            (Material::Unlit, false, Color(0.0, 1.0, 0.0, 1.0)),
        );
        // Placed in its own frame, where layout put it on the wall.
        let inner = surface(
            &plane,
            Mat4::scaling(Vec3::new(64.0, 32.0, 1.0))
                .then(&Mat4::translation(Vec3::new(0.0, 0.0, -100.0))),
            (Material::Unlit, false),
        );
        Box(blue_wall(&plane), BoxSpec::default(), move || {
            // Laid out where it would lie flat on the wall, behind the
            // ball.
            Box(
                Modifier::empty()
                    .absolute_offset(96.0, 48.0)
                    .then(inner.clone())
                    .background(Color(1.0, 0.0, 0.0, 1.0)),
                BoxSpec::default(),
                || {},
            );
        });
    });
}

#[test]
fn a_surface_inside_another_stands_out_of_it_in_the_scene() {
    let Some(frame) = capture(JoinedScene) else {
        return;
    };
    // The panel lies at a depth of 600 - 4 * 100 = 200, nearer than the
    // ball's front at 300.
    let middle = pixel_at(&frame, Vec3::new(0.0, 0.0, 200.0));
    assert!(
        middle[0] > 200 && middle[1] < 40 && middle[2] < 40,
        "the red panel stands in front of the ball: {middle:?}"
    );
    let above = pixel_at(&frame, Vec3::new(0.0, -85.0, 347.0));
    assert!(
        above[1] > 200 && above[0] < 40,
        "the ball shows above the panel: {above:?}"
    );
    let wall = pixel_at(&frame, Vec3::new(-400.0, 0.0, 600.0));
    assert!(
        wall[2] > 200 && wall[0] < 40,
        "the wall shows beside the ball: {wall:?}"
    );
}

/// The nested scene's focal length. Its camera stands this far in front of
/// the nested space's origin, in the doorway on the wall.
const NESTED_FOCAL: f32 = 200.0;

/// A ball of `radius` at `center` in the nested scene's space.
fn nested_ball(center: Vec3, radius: f32) -> Mat4 {
    Mat4::scaling(Vec3::new(radius, radius, radius)).then(&Mat4::translation(center))
}

/// A blue wall far away with a doorway in the middle of its content into a
/// nested scene, which holds a red ball the doorway looks at, a green one
/// behind the nested camera, and a yellow one behind the wall below the
/// doorway. The wall's content pixel is four scene pixels, so a unit of the
/// nested space is four, and the nested point p lies at the scene point
/// (4 p.x, 4 p.y, 1400 + 4 p.z).
#[composable]
fn PortalScene() {
    let (plane, sphere) = meshes();
    Scene(0.0, move || {
        let sphere = Arc::clone(&sphere);
        Box(blue_wall(&plane), BoxSpec::default(), move || {
            let sphere = Arc::clone(&sphere);
            doorway(move || {
                for (center, radius, color) in [
                    (Vec3::new(0.0, 0.0, 100.0), 20.0, Color(1.0, 0.0, 0.0, 1.0)),
                    (Vec3::new(0.0, 0.0, -300.0), 15.0, Color(0.0, 1.0, 0.0, 1.0)),
                    (
                        Vec3::new(0.0, 80.0, -150.0),
                        15.0,
                        Color(1.0, 1.0, 0.0, 1.0),
                    ),
                ] {
                    Surface(
                        Arc::clone(&sphere),
                        nested_ball(center, radius),
                        (Material::Unlit, false, color),
                    );
                }
            });
        });
    });
}

/// The doorway in the middle of the wall's content into the nested scene,
/// whose surfaces `content` composes.
fn doorway(content: impl FnMut() + 'static) {
    Box(
        Modifier::empty()
            .absolute_offset(64.0, 32.0)
            .size(Size {
                width: 128.0,
                height: 64.0,
            })
            .scene3d(|| {
                Scene3d::new(Camera::look_at(
                    Vec3::new(0.0, 0.0, -NESTED_FOCAL),
                    Vec3::ZERO,
                    Vec3::new(0.0, 1.0, 0.0),
                    NESTED_FOCAL,
                ))
            }),
        BoxSpec::default(),
        content,
    );
}

#[test]
fn a_scene_inside_a_wall_shows_its_space_behind_the_wall_through_its_doorway() {
    // Each pipeline the scene draws with has a variant of four samples.
    for samples in [Multisample::Off, Multisample::X4] {
        let Some(frame) = capture_with(PortalScene, samples) else {
            return;
        };
        // The red ball's front lies at 1400 + 4 * 80. The green ball lies at
        // 200, in front of the wall, behind the nested camera.
        let through = pixel_at(&frame, Vec3::new(0.0, 0.0, 1720.0));
        assert!(
            through[0] > 200 && through[1] < 40 && through[2] < 40,
            "the red ball shows through the doorway, and the green one behind the nested camera is \
         cut: {through:?}"
        );
        let wall = pixel_at(&frame, Vec3::new(-400.0, 0.0, 600.0));
        assert!(
            wall[2] > 200 && wall[0] < 40,
            "the wall shows beside the window: {wall:?}"
        );
        // The yellow ball lies behind the wall below its bottom edge, where the
        // eye sees past the wall but not through the doorway.
        let below = pixel_at(&frame, Vec3::new(0.0, 320.0, 800.0));
        assert!(
            below[0] < 40 && below[1] < 40,
            "the space behind the doorway shows only through it: {below:?}"
        );
    }
}

/// The red ball of [`PortalScene`] behind its doorway, coloured by
/// [`FACING_WGSL`].
#[composable]
fn ShadedPortalScene() {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let (plane, sphere) = meshes();
    Scene(0.0, move || {
        let sphere = Arc::clone(&sphere);
        Box(blue_wall(&plane), BoxSpec::default(), move || {
            let sphere = Arc::clone(&sphere);
            doorway(move || {
                Surface(
                    Arc::clone(&sphere),
                    nested_ball(Vec3::new(0.0, 0.0, 100.0), 20.0),
                    (
                        shaded_of(&SHADER, FACING_WGSL),
                        false,
                        Color(1.0, 0.0, 0.0, 1.0),
                    ),
                );
            });
        });
    });
}

#[test]
fn a_shaded_surface_behind_a_doorway_takes_its_colour_from_its_shader() {
    // Each pipeline the scene draws with has a variant of four samples.
    for samples in [Multisample::Off, Multisample::X4] {
        let Some(frame) = capture_with(ShadedPortalScene, samples) else {
            return;
        };
        let through = pixel_at(&frame, Vec3::new(0.0, 0.0, 1720.0));
        assert!(
            through[0] > 240 && through[1] > 200 && through[2] > 240,
            "the ball behind the doorway faces the eye in a space without light, all of which \
         reaches it: {through:?}"
        );
    }
}

/// [`GlassScene`] behind the doorway of [`PortalScene`]: a clear glass ball
/// whose middle lies at the scene point (0, 0, 1400) in front of a red and
/// blue wall at 1800, and a glow panel over the wall's red half, below the
/// ball, at [`PORTAL_GLOW`].
#[composable]
fn PortalGlassScene() {
    let (plane, sphere) = meshes();
    Scene(0.0, move || {
        let (inner_plane, sphere) = (Arc::clone(&plane), Arc::clone(&sphere));
        Box(blue_wall(&plane), BoxSpec::default(), move || {
            let (plane, sphere) = (Arc::clone(&inner_plane), Arc::clone(&sphere));
            doorway(move || {
                Surface(
                    Arc::clone(&plane),
                    panel((50.0, 10.0), Vec3::new(-75.0, 42.5, 75.0)),
                    (Material::Glow, false, Color(0.0, 0.5, 0.0, 1.0)),
                );
                Surface(
                    Arc::clone(&sphere),
                    nested_ball(Vec3::ZERO, SPHERE_RADIUS / 4.0),
                    (
                        Material::Glass(Glass::clear(SPHERE_RADIUS / 2.0)),
                        false,
                        Color::TRANSPARENT,
                    ),
                );
                red_blue_wall(&plane, panel((500.0, 375.0), Vec3::new(0.0, 0.0, 100.0)));
            });
        });
    });
}

/// Where the glow panel of [`PortalGlassScene`] shows, in scene points.
const PORTAL_GLOW: Vec3 = Vec3::new(-300.0, 170.0, 1700.0);

#[test]
fn glass_and_glow_show_through_a_doorway() {
    // Each pipeline the scene draws with has a variant of four samples.
    for samples in [Multisample::Off, Multisample::X4] {
        let Some(frame) = capture_with(PortalGlassScene, samples) else {
            return;
        };
        let middle = Vec3::new(0.0, 0.0, 1400.0);
        let ball_left = pixel_at(
            &frame,
            middle + Vec3::new(-0.5, 0.0, -0.866) * SPHERE_RADIUS,
        );
        let ball_right = pixel_at(&frame, middle + Vec3::new(0.5, 0.0, -0.866) * SPHERE_RADIUS);
        assert!(
            ball_left[2] > ball_left[0] + 60,
            "the ball's left side shows the blue right half of the wall behind it: {ball_left:?}"
        );
        assert!(
            ball_right[0] > ball_right[2] + 60,
            "the ball's right side shows the red left half of the wall behind it: {ball_right:?}"
        );
        let glow = pixel_at(&frame, PORTAL_GLOW);
        assert!(
            glow[0] > 200 && glow[1] > 100,
            "the glow panel adds its green to the red wall behind the doorway: {glow:?}"
        );
    }
}

/// A camera at `eye` looking along `ahead`, as the walk test's eye.
fn walker(eye: Vec3, ahead: Vec3) -> Camera {
    Camera::look_at(eye, eye + ahead, Vec3::new(0.0, 1.0, 0.0), FOCAL_LENGTH)
}

/// The frame pixel at the scene point `point` as `camera` sees it.
fn pixel_seen(frame: &cranpose_render_wgpu::CapturedFrame, camera: Camera, point: Vec3) -> [u8; 4] {
    let shown = camera
        .project(
            point,
            Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            },
        )
        .expect("in front of the eye");
    let index = (shown.y as usize * frame.width as usize + shown.x as usize) * 4;
    std::array::from_fn(|channel| frame.pixels[index + channel])
}

/// Where the walk test's eye starts: past the room's green panel, before
/// the wall.
const WALK_START: Vec3 = Vec3::new(0.0, 0.0, 500.0);

/// The doorway scene of [`PortalScene`] with only its red ball behind the
/// doorway, a green panel in the room in front of the wall, and an eye the
/// test walks.
#[composable]
fn WalkScene() {
    let (plane, sphere) = meshes();
    let eye = cranpose_ui::rememberEye(|| EyePose::standing(WALK_START, 0.0, 0.0), 10.0);
    WALKER.with(|slot| *slot.borrow_mut() = Some(eye.clone()));
    Box(
        Modifier::empty()
            .size(Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            })
            .background(Color::BLACK)
            .scene3d(move || eye.scene(FOCAL_LENGTH, None)),
        BoxSpec::default(),
        move || {
            Surface(
                Arc::clone(&plane),
                panel((400.0, 400.0), Vec3::new(0.0, 0.0, -200.0)),
                (Material::Unlit, false, Color(0.0, 1.0, 0.0, 1.0)),
            );
            let sphere = Arc::clone(&sphere);
            Box(blue_wall(&plane), BoxSpec::default(), move || {
                let sphere = Arc::clone(&sphere);
                doorway(move || {
                    Surface(
                        Arc::clone(&sphere),
                        nested_ball(Vec3::new(0.0, 0.0, 100.0), 20.0),
                        (Material::Unlit, false, Color(1.0, 0.0, 0.0, 1.0)),
                    );
                });
            });
        },
    );
}

#[test]
fn an_eye_that_walks_through_a_doorway_sees_the_space_behind_it_and_the_room_back_through_it() {
    let Some((_lock, mut shell)) = support::app_shell_for(
        WalkScene,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    ) else {
        return;
    };
    support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let eye = WALKER.with(|slot| slot.borrow().clone().expect("the scene composed"));
    // The eye walks through the doorway's middle, on the wall at 600, into
    // the space behind it, where it stands short of the red ball's front at
    // 1720.
    let inside = Vec3::new(0.0, 0.0, 1500.0);
    eye.walk(inside - WALK_START);
    assert_eq!(
        eye.pose().doorways.len(),
        1,
        "the eye stands in the space behind the doorway"
    );
    let ahead = walker(inside, Vec3::new(0.0, 0.0, 1.0));
    support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let (_, frame) = support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let front = pixel_seen(&frame, ahead, Vec3::new(0.0, 0.0, 1720.0));
    assert!(
        front[0] > 200 && front[1] < 40,
        "past the doorway the eye sees the red ball in the space behind it: {front:?}"
    );
    let back = walker(inside, Vec3::new(0.0, 0.0, -1.0));
    eye.turn(std::f32::consts::PI, 0.0);
    support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let (_, frame) = support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let room = pixel_seen(&frame, back, Vec3::new(0.0, 0.0, -200.0));
    assert!(
        room[1] > 200 && room[2] < 40,
        "turned around, the eye sees the room's green panel back through the doorway: {room:?}"
    );
    let beside = pixel_seen(&frame, back, Vec3::new(0.0, 200.0, 600.0));
    assert!(
        beside[2] < 40,
        "beside the doorway the wall of the room the eye left does not show: {beside:?}"
    );
}

#[test]
fn an_eye_nearer_to_a_doorway_than_the_near_plane_sees_through_it() {
    let Some((_lock, mut shell)) = support::app_shell_for(
        WalkScene,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    ) else {
        return;
    };
    support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let eye = WALKER.with(|slot| slot.borrow().clone().expect("the scene composed"));
    // The eye stops in the doorway's middle, closer to the wall at 600 than
    // the camera's near plane, a sixty-fourth of its focal length, lies.
    let close = Vec3::new(0.0, 0.0, 600.0 - FOCAL_LENGTH * NEAR_PLANE * 0.5);
    eye.walk(close - WALK_START);
    support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let (_, frame) = support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    let front = pixel_seen(
        &frame,
        walker(close, Vec3::new(0.0, 0.0, 1.0)),
        Vec3::new(0.0, 0.0, 1720.0),
    );
    assert!(
        front[0] > 200 && front[1] < 40,
        "the doorway nearer than the near plane shows the red ball behind it: {front:?}"
    );
}

/// The doorway of [`PortalScene`], and behind it, `depth` times, a wall
/// across the nested space with a doorway of its own, a nested unit to a
/// content pixel, and at the end a red ball the last doorway looks at.
fn deep_doorway(plane: Arc<Mesh>, sphere: Arc<Mesh>, depth: usize) {
    doorway(move || {
        if depth == 0 {
            Surface(
                Arc::clone(&sphere),
                nested_ball(Vec3::new(0.0, 0.0, 100.0), 20.0),
                (Material::Unlit, false, Color(1.0, 0.0, 0.0, 1.0)),
            );
            return;
        }
        let wall = surface(
            &plane,
            panel((256.0, 128.0), Vec3::new(0.0, 0.0, 100.0)),
            (Material::Unlit, false),
        )
        .background(Color(0.0, 0.0, 1.0, 1.0));
        let (plane, sphere) = (Arc::clone(&plane), Arc::clone(&sphere));
        Box(wall, BoxSpec::default(), move || {
            deep_doorway(Arc::clone(&plane), Arc::clone(&sphere), depth - 1);
        });
    });
}

/// A blue wall with five doorways one inside another behind it, and a red
/// ball behind the last.
#[composable]
fn DeepScene() {
    let (plane, sphere) = meshes();
    Scene(0.0, move || {
        let (inner_plane, sphere) = (Arc::clone(&plane), Arc::clone(&sphere));
        Box(blue_wall(&plane), BoxSpec::default(), move || {
            deep_doorway(Arc::clone(&inner_plane), Arc::clone(&sphere), 4);
        });
    });
}

#[test]
fn doorways_inside_doorways_show_as_deep_as_they_open_on_screen() {
    let Some(frame) = capture(DeepScene) else {
        return;
    };
    let index = ((FRAME_HEIGHT / 2 * frame.width + FRAME_WIDTH / 2) * 4) as usize;
    let middle: [u8; 4] = std::array::from_fn(|channel| frame.pixels[index + channel]);
    assert!(
        middle[0] > 200 && middle[1] < 40 && middle[2] < 40,
        "the red ball behind the fifth doorway shows through all five: {middle:?}"
    );
}

// A white sheet turned in its plane on black, its texture's edges given as
// `edges`.
#[composable]
fn SlantedSheet(edges: TextureEdges) {
    let sheet =
        remember(move || Arc::new(Mesh::plane().with_texture_edges(edges))).with(Arc::clone);
    let transform = panel((300.0, 200.0), Vec3::ZERO).then(&Mat4::rotation_z(0.2));
    Scene(0.0, move || {
        Surface(
            Arc::clone(&sheet),
            transform,
            (Material::Unlit, false, Color::WHITE),
        );
    });
}

#[composable]
fn OutlineSheetScene() {
    SlantedSheet(TextureEdges::Outline);
}

#[composable]
fn SeamsSheetScene() {
    SlantedSheet(TextureEdges::Seams);
}

// How many pixels of the frame are neither the black around the sheet nor
// the sheet's white: those an edge covers in part.
fn partly_covered(frame: &cranpose_render_wgpu::CapturedFrame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| (24..232).contains(&pixel[0]))
        .count()
}

#[test]
fn a_sheet_whose_outline_is_its_texture_edge_draws_a_smooth_outline() {
    let (Some(outline), Some(seams)) = (capture(OutlineSheetScene), capture(SeamsSheetScene))
    else {
        return;
    };
    let (smooth, stepped) = (partly_covered(&outline), partly_covered(&seams));
    assert!(
        smooth > 300,
        "the outline's pixels are covered in part along its four slanted edges: {smooth}"
    );
    assert!(
        stepped < smooth / 10,
        "a mesh of seams draws its triangles' pixels whole: {stepped}"
    );
    let Some(sampled) = capture_with(OutlineSheetScene, Multisample::X4) else {
        return;
    };
    let sampled = partly_covered(&sampled);
    assert!(
        sampled > 300,
        "with four samples the outline still fades over its pixels: {sampled}"
    );
}
