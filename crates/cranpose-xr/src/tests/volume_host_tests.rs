// The scene of the volumes as a headset runs it, on the GPU of the machine: one composition holds
// the volumes' content, and one pass draws it once for each eye, from that eye's view.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use cranpose::{
    AppLauncher, AppSettings, GpuContext, GuestView, SCENE_DEPTH_FORMAT, gpu_context,
    prelude::*,
    space::{Light, Mat4, Material, Mesh, MeshSurface, Multisample, Vec3},
};
use cranpose_core::{CompositionLocalProvider, LaunchedEffect, rememberMutableStateOf};

use super::*;
use crate::{
    FieldOfView, Pose, RenderViewpoint, SpatialVolume, Subspace, SubspaceModifier,
    eye_camera::{EyeCamera, FirstRow, ImageRegion},
    space::{Space, local_space},
    volume_pass::{EyeImage, VolumePass},
};

const EYE: u32 = 128;
// The quad's side in dp, which are millimeters: 60 cm
const QUAD: f32 = 600.0;
// The size of the content that the quad shows, in pixels
const CONTENT: f32 = 32.0;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[cfg(feature = "gltf")]
#[path = "volume_gltf_tests.rs"]
mod gltf;

#[path = "volume_antialias_tests.rs"]
mod antialias;

#[path = "volume_picking_tests.rs"]
mod picking;

#[path = "controller_scene_tests.rs"]
mod controllers;

#[path = "projected_pass_tests.rs"]
mod projected;

#[path = "spatial_tray_tests.rs"]
mod tray;

// What the host of a test holds: the GPU, the guest view that runs the content, the space and the
// counts of what composed.
struct Headset {
    gpu: GpuContext,
    guest: GuestView,
    space: Space,
    shown: MutableState<bool>,
    compositions: Rc<Cell<u32>>,
    // The value of an animated state at each evaluation of the scene's mesh
    seen: Rc<RefCell<Vec<u32>>>,
}

fn gpu() -> GpuContext {
    let found = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&found);
    // A component that composes on a device of its own shows that device's context
    let _host = AppLauncher::new()
        .create_embedded_view(16, 16, 1.0, move || {
            slot.replace(gpu_context());
        })
        .expect("GPU embedded component");
    found.take().expect("a GPU context")
}

// What the volume of a test reports to the test.
#[derive(Clone)]
struct Probes {
    compositions: Rc<Cell<u32>>,
    // The value of an animated state at each evaluation of the scene's mesh
    seen: Rc<RefCell<Vec<u32>>>,
}

// What a test composes in the subspace of a headset: it gets the probes and a state that advances
// once for each frame of the host.
type VolumeComposer = Rc<dyn Fn(&Probes, MutableState<u32>)>;

// A headset with one volume: a red quad of `QUAD` dp a meter ahead, in the plane of the volume's
// center, which stands in a subspace of the app's content.
fn headset() -> Headset {
    let plane = Arc::new(Mesh::plane());
    headset_of(Rc::new(move |probes, ticks| {
        quad_volume(
            Rc::clone(&probes.compositions),
            Arc::clone(&plane),
            (ticks, Rc::clone(&probes.seen)),
        );
    }))
}

// A headset whose subspace holds what `volume` composes, with a state that advances each frame.
fn headset_of(volume: VolumeComposer) -> Headset {
    headset_with(volume, Ticks::EachFrame)
}

// Whether a headset's composition keeps a state that advances each frame, and so asks for every
// frame, or asks only for the frames that its content asks for.
#[derive(Clone, Copy, PartialEq)]
enum Ticks {
    EachFrame,
    None,
}

fn headset_with(volume: VolumeComposer, ticking: Ticks) -> Headset {
    let gpu = gpu();
    let probes = Probes {
        compositions: Rc::new(Cell::new(0)),
        seen: Rc::new(RefCell::new(Vec::new())),
    };
    let reported = probes.clone();
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let guest = GuestView::new(
        &AppSettings::default(),
        &gpu,
        COLOR,
        16,
        16,
        1.0,
        move || {
            let space = remember(|| Space::with_scene_size([EYE as f32; 2])).with(Clone::clone);
            let shown = rememberMutableStateOf(|| true);
            keep.replace(Some((space.clone(), shown)));
            let ticks = rememberMutableStateOf(|| 0_u32);
            // A headset with no ticks asks for no frame of its own
            if ticking == Ticks::EachFrame {
                LaunchedEffect((), async move |scope| {
                    loop {
                        scope.with_frame_nanos(|now| now).await;
                        ticks.update(|ticks| *ticks += 1);
                    }
                });
            }
            let (probes, volume, host) = (probes.clone(), Rc::clone(&volume), space.clone());
            CompositionLocalProvider([local_space().provides(Some(space))], move || {
                let (probes, volume) = (probes.clone(), Rc::clone(&volume));
                Subspace(move || {
                    if shown.get() {
                        volume(&probes, ticks);
                    }
                });
                VolumeScene(host);
            });
        },
    )
    .expect("a guest view");
    let (space, shown) = held.take().expect("the content composed");
    Headset {
        gpu,
        guest,
        space,
        shown,
        compositions: reported.compositions,
        seen: reported.seen,
    }
}

#[composable]
fn quad_volume(
    compositions: Rc<Cell<u32>>,
    plane: Arc<Mesh>,
    (ticks, seen): (MutableState<u32>, Rc<RefCell<Vec<u32>>>),
) {
    SpatialVolume(
        SubspaceModifier::new().size(QUAD, QUAD).depth(10.0),
        move |scope| {
            let seen = Rc::clone(&seen);
            scope.light(Light {
                direction: Vec3::new(0.0, 0.0, 1.0),
                ambient: 1.0,
                shadows: false,
                size: 0.0,
            });
            compositions.set(compositions.get() + 1);
            let plane = Arc::clone(&plane);
            Box(
                Modifier::empty()
                    .required_size(Size::new(CONTENT, CONTENT))
                    .mesh_surface(move || {
                        // The scene reads the animation while it draws
                        seen.borrow_mut().push(ticks.get());
                        MeshSurface {
                            mesh: Arc::clone(&plane),
                            // The mesh's unit square, scaled and moved so that it surrounds the
                            // volume's center
                            transform: Mat4::scaling(Vec3::new(QUAD, QUAD, 1.0))
                                .then(&Mat4::translation(Vec3::new(-QUAD / 2.0, -QUAD / 2.0, 0.0))),
                            material: Material::Emissive,
                            casts_shadow: false,
                            solid: true,
                        }
                    })
                    .background(Color(1.0, 0.0, 0.0, 1.0)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

// An eye `x` meters to the right of the head's middle, that looks straight ahead with a view of 90
// degrees each way.
fn eye(x: f32) -> RenderViewpoint {
    eye_at([x, 0.0, 0.0])
}

// An eye at `position`, in meters from the head's middle, that looks straight ahead with a view of
// 90 degrees each way.
fn eye_at(position: [f32; 3]) -> RenderViewpoint {
    let quarter = std::f32::consts::FRAC_PI_4;
    RenderViewpoint {
        pose: Pose::at(position),
        local_pose: Pose::at(position),
        field_of_view: FieldOfView {
            angle_left: -quarter,
            angle_right: quarter,
            angle_up: quarter,
            angle_down: -quarter,
        },
    }
}

// The textures of one eye's picture and its depth.
struct Eye {
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
}

impl Eye {
    fn new(gpu: &GpuContext) -> Self {
        Self::sized(gpu, (EYE, EYE))
    }

    fn sized(gpu: &GpuContext, (width, height): (u32, u32)) -> Self {
        let texture = |format, usage| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("eye"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            COLOR,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            SCENE_DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        Self {
            color_view: color.create_view(&wgpu::TextureViewDescriptor::default()),
            depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
            color,
            depth,
        }
    }

    fn image(&self) -> EyeImage<'_> {
        EyeImage {
            color: &self.color,
            color_view: &self.color_view,
            depth: Some(&self.depth_view),
        }
    }
}

// Draws the scene for `view` into `eye`, again until a frame drawn from `view` itself took its own
// pipeline for every draw. Returns whether the pass found a scene.
fn draw(headset: &mut Headset, pass: &mut VolumePass, view: &RenderViewpoint, eye: &Eye) -> bool {
    draw_through(headset, pass, (view, eye), |moved| {
        EyeCamera::of(moved, (EYE, EYE), FirstRow::Top).expect("a camera")
    })
}

// Draws the scene for `view` into `eye` through the camera that `camera` makes of the view, as
// `draw` does.
fn draw_through(
    headset: &mut Headset,
    pass: &mut VolumePass,
    (view, eye): (&RenderViewpoint, &Eye),
    camera: impl Fn(&RenderViewpoint) -> EyeCamera,
) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut attempt = 0_u32;
    loop {
        // A head is never still: the odd frames move the eye by a hair, and an even one draws
        // from `view` after a moved one, whatever frame the last call ended on.
        attempt = attempt.wrapping_add(1);
        let at_view = attempt & 1 == 0;
        let mut moved = *view;
        moved.pose.position[1] += if at_view { 0.0 } else { 1e-5 };
        let camera = camera(&moved);
        let drew = pass
            .draw(&mut headset.guest, &headset.space, &camera, &eye.image())
            .expect("a frame");
        if !drew || (at_view && pipelines_settled(&mut headset.guest, pass)) {
            return drew;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the scene's pipelines never landed"
        );
        std::thread::yield_now();
    }
}

// Whether the last frame of the scene's window took its own pipeline for every draw, as its
// renderer counts it.
fn pipelines_settled(guest: &mut GuestView, pass: &VolumePass) -> bool {
    pass.root()
        .and_then(|root| {
            guest
                .shell()
                .surface(cranpose_app_shell::RootId::Window(root))
        })
        .and_then(|mut surface| surface.renderer().last_frame_stats())
        .is_some_and(|stats| stats.pipelines_settled())
}

// Whether any pixel of `color` holds a colour or covers what lies behind it: a surface that glows
// adds its colour with no alpha.
fn shows_something(gpu: &GpuContext, color: &wgpu::Texture) -> bool {
    read(gpu, color, 4).iter().any(|&channel| channel != 0)
}

// The box of the pixels that the scene drew, as the first and last column and row.
fn drawn_box(pixels: &[u8]) -> Option<[u32; 4]> {
    let mut found: Option<[u32; 4]> = None;
    for (at, texel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if texel[3] == 0 {
            continue;
        }
        let at = u32::try_from(at).expect("a pixel of the eye");
        let (x, y) = (at % EYE, at / EYE);
        let [left, top, right, bottom] = found.get_or_insert([x, y, x, y]);
        (*left, *top, *right, *bottom) = (
            (*left).min(x),
            (*top).min(y),
            (*right).max(x),
            (*bottom).max(y),
        );
    }
    found
}

// The first column, counted from the left, in which the middle row of `color` is red.
fn first_red_column(gpu: &GpuContext, color: &wgpu::Texture) -> Option<u32> {
    let pixels = read(gpu, color, 4);
    let row = (EYE / 2 * EYE * 4) as usize;
    (0..EYE).find(|column| {
        let at = row + *column as usize * 4;
        pixels[at] > 200 && pixels[at + 1] < 50 && pixels[at + 3] == 255
    })
}

// The bytes of `texture` row by row, `texel` bytes to the pixel, without the padding of a row.
fn read(gpu: &GpuContext, texture: &wgpu::Texture, texel: u32) -> Vec<u8> {
    let padded = (texture.width() * texel).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let (width, height) = (texture.width(), texture.height());
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut copy = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(padded),
        rows_per_image: Some(height),
    };
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    copy.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout,
        },
        extent,
    );
    gpu.queue.submit([copy.finish()]);
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, |mapped| mapped.expect("map"));
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let bytes = staging.slice(..).get_mapped_range().expect("mapped range");
    let used = (width * texel) as usize;
    let mut rows = Vec::with_capacity(used * height as usize);
    for row in bytes.chunks_exact(padded as usize) {
        rows.extend_from_slice(&row[..used]);
    }
    rows
}

// The depth that the scene left, as a float for each pixel, read through a shader because a depth
// texture of this format cannot be copied to a buffer.
fn read_depth(gpu: &GpuContext, depth: &wgpu::Texture) -> Vec<f32> {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth readback"),
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var depth: texture_depth_2d;
             @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                 let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
                 return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
             }
             @fragment fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
                 return vec4<f32>(textureLoad(depth, vec2<i32>(at.xy), 0), 0.0, 0.0, 1.0);
             }"
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("depth readback"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba32Float.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth readback"),
        size: wgpu::Extent3d {
            width: EYE,
            height: EYE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&depth.create_view(
                &wgpu::TextureViewDescriptor {
                    aspect: wgpu::TextureAspect::DepthOnly,
                    ..Default::default()
                },
            )),
        }],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    gpu.queue.submit([encoder.finish()]);
    read(gpu, &target, 16)
        .as_chunks::<16>()
        .0
        .iter()
        .map(|texel| f32::from_le_bytes([texel[0], texel[1], texel[2], texel[3]]))
        .collect()
}

#[test]
fn one_composition_draws_the_volumes_for_each_eye_from_that_eyes_view_with_depth() {
    let mut headset = headset();
    let mut pass = VolumePass::default();
    let (left, right) = (Eye::new(&headset.gpu), Eye::new(&headset.gpu));
    assert!(draw(&mut headset, &mut pass, &eye(-0.032), &left));
    assert!(draw(&mut headset, &mut pass, &eye(0.032), &right));

    // Each eye sees the quad from its own place: the nearer an object, the farther apart the two
    // pictures put it. A quad a meter ahead of eyes 64 mm apart, in a view that spans 128 pixels
    // for 90 degrees, shifts by 4 pixels between the eyes.
    let first_left = first_red_column(&headset.gpu, &left.color).expect("the left eye sees it");
    let first_right = first_red_column(&headset.gpu, &right.color).expect("the right eye sees it");
    assert!(
        (3..=5).contains(&(first_left - first_right)),
        "the left eye's quad starts at column {first_left}, the right eye's at {first_right}"
    );
    // The scene composed the content once, and drawing it twice composed nothing
    assert_eq!(headset.compositions.get(), 1);
    assert_quad_depth(&headset.gpu, &left, &eye(-0.032), "the left eye");
}

// Checks that `image` holds the depth that the quad leaves as `view` sees it. The middle holds
// 1 - near / z, where `z` is the quad's distance along the eye's axis. A corner, where no scene
// drew, holds 1.
fn assert_quad_depth(gpu: &GpuContext, image: &Eye, view: &RenderViewpoint, what: &str) {
    let near = EyeCamera::of(view, (EYE, EYE), FirstRow::Top)
        .expect("a camera")
        .camera
        .near();
    // The quad stands a meter ahead of the head's middle, and a dp is a millimeter
    let expected = 1.0 - near / ((1.0 + view.pose.position[2]) * 1000.0);
    let depth = read_depth(gpu, &image.depth);
    let at = |x: u32, y: u32| depth[(y * EYE + x) as usize];
    let middle = at(EYE / 2, EYE / 2);
    assert!(
        (middle - expected).abs() < 1e-4,
        "{what}: the middle holds the depth {middle}, the quad's is {expected}"
    );
    assert!(
        (at(2, 2) - 1.0).abs() < 1e-6,
        "{what}: a corner holds the depth {}",
        at(2, 2)
    );
}

// Draws the scene for `view` into `image` as `draw` does, again until the middle of its depth shows
// the quad: the pass that leaves the depth of a scene of four samples has a pipeline of its own,
// which may land after the colour's.
fn draw_with_depth(
    headset: &mut Headset,
    pass: &mut VolumePass,
    view: &RenderViewpoint,
    image: &Eye,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        assert!(draw(headset, pass, view, image));
        if read_depth(&headset.gpu, &image.depth)[(EYE * EYE / 2 + EYE / 2) as usize] < 1.0 {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "no depth was written");
    }
}

// A runtime's depth swapchain hands out an image whose depth the renderer did not write, while the
// head holds still. The frame changes nothing in the scene. The image must still hold the scene's
// depth.
#[test]
fn a_frame_that_changes_nothing_leaves_the_scenes_depth_in_a_new_depth_image() {
    for samples in [Multisample::Off, Multisample::X4] {
        let mut headset = headset();
        headset.space.volumes().samples().set(samples);
        let mut pass = VolumePass::default();
        let (first, next) = (Eye::new(&headset.gpu), Eye::new(&headset.gpu));
        let view = eye(0.0);
        draw_with_depth(&mut headset, &mut pass, &view, &first);
        assert!(draw_once_from(&mut headset, &mut pass, &view, &next));
        assert_quad_depth(
            &headset.gpu,
            &next,
            &view,
            &format!("{samples:?}, the next image"),
        );
    }
}

// The swapchain hands out an image that holds the depth of an older frame, from another place of
// the head, while the head holds still at its new place.
#[test]
fn a_frame_that_changes_nothing_writes_its_depth_over_an_image_of_an_older_frame() {
    for samples in [Multisample::Off, Multisample::X4] {
        let mut headset = headset();
        headset.space.volumes().samples().set(samples);
        let mut pass = VolumePass::default();
        let (older, newer) = (Eye::new(&headset.gpu), Eye::new(&headset.gpu));
        let (back, ahead) = (eye_at([0.0, 0.0, 0.25]), eye_at([0.0, 0.0, -0.25]));
        draw_with_depth(&mut headset, &mut pass, &back, &older);
        draw_with_depth(&mut headset, &mut pass, &ahead, &newer);
        assert!(draw_once_from(&mut headset, &mut pass, &ahead, &older));
        assert_quad_depth(
            &headset.gpu,
            &older,
            &ahead,
            &format!("{samples:?}, the older image"),
        );
    }
}

#[test]
fn a_region_of_the_view_draws_what_that_part_of_the_whole_view_shows() {
    let mut headset = headset();
    let mut pass = VolumePass::default();
    let view = eye(0.0);
    let whole = Eye::new(&headset.gpu);
    assert!(draw(&mut headset, &mut pass, &view, &whole));
    // A smaller image of the part of the view that starts 32 columns and 40 rows into the whole
    // view's image, at the same pixels for a unit of tangent: the quad lies inside it
    let full = ImageRegion::whole(&view.field_of_view, (EYE, EYE)).expect("a region");
    let (columns, rows) = (32_u32, 40_u32);
    let region = ImageRegion {
        left: full.left + columns as f32 / full.focal_length,
        top: full.top - rows as f32 / full.focal_length,
        focal_length: full.focal_length,
    };
    let size = (64, 48);
    let part = Eye::sized(&headset.gpu, size);
    assert!(draw_through(
        &mut headset,
        &mut pass,
        (&view, &part),
        |moved| { EyeCamera::of_region(moved, (region, size), (EYE, EYE), FirstRow::Top) }
    ));
    let whole_pixels = read(&headset.gpu, &whole.color, 4);
    let part_pixels = read(&headset.gpu, &part.color, 4);
    let mut red = 0;
    for y in 0..size.1 {
        for x in 0..size.0 {
            let shown = &part_pixels[((y * size.0 + x) * 4) as usize..][..4];
            let expected = &whole_pixels[(((y + rows) * EYE + x + columns) * 4) as usize..][..4];
            assert!(
                shown
                    .iter()
                    .zip(expected)
                    .all(|(shown, expected)| shown.abs_diff(*expected) <= 2),
                "pixel ({x}, {y}) of the part is {shown:?}, the whole view's is {expected:?}"
            );
            red += usize::from(shown[0] > 200);
        }
    }
    assert!(
        red > 1000,
        "the part shows the whole quad: {red} red pixels"
    );
}

#[test]
fn a_frame_with_no_volume_draws_no_scene() {
    let mut headset = headset();
    let mut pass = VolumePass::default();
    let eyes = (Eye::new(&headset.gpu), Eye::new(&headset.gpu));
    assert_eq!(headset.space.volume_count(), 1);
    assert!(draw(&mut headset, &mut pass, &eye(-0.032), &eyes.0));

    // The app removes its volume. The next frames compose that, and the scene has no window root
    headset.shown.set(false);
    settle_volumes(&mut headset, 0);
    assert!(
        !draw_once(&mut headset, &mut pass, &eyes.0),
        "the scene drew with no volume"
    );
}

#[test]
fn a_held_scene_keeps_its_window_with_no_volume_and_lets_it_go_when_the_hold_ends() {
    let mut headset = headset();
    let mut pass = VolumePass::default();
    let image = Eye::new(&headset.gpu);
    assert!(draw(&mut headset, &mut pass, &eye(0.0), &image));
    let root = pass.root().expect("the scene's window root");

    // The frames hold the scene, as a headset's frames do a while after its last content went
    headset.space.volumes().held().set(true);
    headset.shown.set(false);
    settle_volumes(&mut headset, 0);
    assert!(
        draw_once(&mut headset, &mut pass, &image),
        "the held scene lost its window"
    );
    assert_eq!(
        pass.root(),
        Some(root),
        "the held scene made its window again"
    );

    assert!(
        headset.guest.visit_window_meshes((root, root), &mut |_| {}),
        "the held scene's window lost its view"
    );

    // The hold ends: the frames release the pass, whose view goes at once
    headset.space.volumes().held().set(false);
    pass.release(&mut headset.guest);
    assert!(
        !headset.guest.visit_window_meshes((root, root), &mut |_| {}),
        "the view stayed after the hold"
    );
    headset.guest.shell().update();
    assert!(
        !draw_once(&mut headset, &mut pass, &image),
        "the scene kept its window after the hold"
    );

    // A volume that comes back after the hold draws in a window of its own
    headset.shown.set(true);
    settle_volumes(&mut headset, 1);
    assert!(draw(&mut headset, &mut pass, &eye(0.0), &image));
    assert!(shows_something(&headset.gpu, &image.color));
}

// The frames compose until the space has `count` volumes, and once more for the scene.
fn settle_volumes(headset: &mut Headset, count: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while headset.space.volume_count() != count {
        assert!(
            std::time::Instant::now() < deadline,
            "the volumes stayed as they were"
        );
        headset.guest.shell().update();
    }
    headset.guest.shell().update();
}

// Draws the scene once from the middle of the head. Returns whether the pass found a scene.
fn draw_once(headset: &mut Headset, pass: &mut VolumePass, image: &Eye) -> bool {
    draw_once_from(headset, pass, &eye(0.0), image)
}

// Draws the scene once for `view`. Returns whether the pass found a scene.
fn draw_once_from(
    headset: &mut Headset,
    pass: &mut VolumePass,
    view: &RenderViewpoint,
    image: &Eye,
) -> bool {
    let camera = EyeCamera::of(view, (EYE, EYE), FirstRow::Top).expect("a camera");
    pass.draw(&mut headset.guest, &headset.space, &camera, &image.image())
        .expect("a frame")
}

#[test]
fn a_volume_that_moves_draws_again_where_it_stands_without_composing() {
    let mut headset = headset();
    let mut pass = VolumePass::default();
    let eye_image = Eye::new(&headset.gpu);
    let view = eye(0.0);
    assert!(draw(&mut headset, &mut pass, &view, &eye_image));
    let before = first_red_column(&headset.gpu, &eye_image.color).expect("the quad shows");
    // The user carries the volume 20 cm to the right: the scene reads the pose while it draws
    let content = headset.space.volumes().contents().get().remove(0);
    let moved = Pose::at([0.2, 0.0, -1.0]);
    content.pose.set(moved);
    assert!(draw(&mut headset, &mut pass, &view, &eye_image));
    let after = first_red_column(&headset.gpu, &eye_image.color).expect("the quad shows");
    // 20 cm at a meter is 12.8 pixels in this view
    assert!((12..=14).contains(&(after - before)), "{before} to {after}");
    assert_eq!(headset.compositions.get(), 1);
}

#[test]
fn a_volume_that_changes_every_frame_does_not_redraw_the_panels_atlas() {
    let mut headset = headset();
    let mut pass = VolumePass::default();
    let eye_image = Eye::new(&headset.gpu);
    let atlas = Eye::new(&headset.gpu);
    let view = eye(0.0);
    assert!(draw(&mut headset, &mut pass, &view, &eye_image));
    // The atlas draws until the composition has settled
    let draw_atlas = |headset: &mut Headset| {
        headset
            .guest
            .draw(&atlas.color, &atlas.color_view)
            .expect("the atlas")
    };
    for _ in 0..200 {
        if !draw_atlas(&mut headset) {
            break;
        }
    }
    assert!(!draw_atlas(&mut headset), "the atlas keeps drawing");
    // The volume moves and the eye's camera changes in each frame: only the scene draws
    let content = headset.space.volumes().contents().get().remove(0);
    for frame in 1..=3 {
        // The frame's atlas update comes first and finds the volume's new pose
        content.pose.set(Pose::at([0.01 * frame as f32, 0.0, -1.0]));
        assert!(
            !draw_atlas(&mut headset),
            "the atlas drew for a change of the volumes' scene"
        );
        assert!(draw(&mut headset, &mut pass, &view, &eye_image));
    }
}

#[test]
fn both_eyes_draw_the_same_state_of_an_animation_that_advances_once_a_frame() {
    let mut headset = headset();
    let mut pass = VolumePass::default();
    let (left, right) = (Eye::new(&headset.gpu), Eye::new(&headset.gpu));
    let atlas = Eye::new(&headset.gpu);
    assert!(draw(&mut headset, &mut pass, &eye(-0.032), &left));
    let camera = |x| EyeCamera::of(&eye(x), (EYE, EYE), FirstRow::Top).expect("a camera");
    let mut frame = |headset: &mut Headset| {
        headset.seen.borrow_mut().clear();
        // The frame starts with the atlas's update, which is the frame's tick, and the scene
        // reads the animation there
        headset
            .guest
            .draw(&atlas.color, &atlas.color_view)
            .expect("the atlas");
        for (x, image) in [(-0.032, &left), (0.032, &right)] {
            pass.draw(
                &mut headset.guest,
                &headset.space,
                &camera(x),
                &image.image(),
            )
            .expect("a frame");
        }
        headset.seen.borrow().clone()
    };
    let mut states = Vec::new();
    for _ in 0..8 {
        let seen = frame(&mut headset);
        // A draw of an eye that ran the frame's callbacks would advance the animation again, and
        // the scene would read the new state
        assert!(
            seen.iter().all(|ticks| *ticks == seen[0]),
            "a frame drew different states of the animation: {seen:?}"
        );
        states.extend(seen.first().copied());
    }
    assert!(
        states.windows(2).any(|pair| pair[0] != pair[1]),
        "the animation stood still: {states:?}"
    );
}
