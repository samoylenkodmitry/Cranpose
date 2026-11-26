//! The projection layer of the app's 3D content: a swapchain of pictures for each eye, and a second
//! one for the depth of those pictures when the runtime takes it.
//!
//! The composition draws the scene of the volumes into the eye's next image from that eye's own
//! pose and field of view, once for each eye in each frame. The scene draws against the depth
//! image, so the compositor receives the depth of what it shows. A frame with more layers than the
//! runtime takes draws the pictures of some layers over the scene, against the same depth. With no
//! volume in the composition, no controller that shows a model and no such picture, no pass runs.
//! The swapchains and the scene's window stay a short while after the last of them goes.

use std::time::Instant;

use cranpose::{GpuContext, GuestView};
use openxr as xr;

use super::{
    graphics::{Api, Graphics, SwapchainFormat},
    handle_layers,
    session::{Alpha, EyeDepth, ProjectionEye, ProjectionLayer, XrSession},
};
use crate::{
    Pose, RenderViewpoint,
    eye_camera::{EyeCamera, FirstRow},
    projected_pass::{EyeTarget, ProjectedLayer, ProjectedPass},
    scene_hold::SceneHold,
    space::Space,
    volume_pass::{EyeImage, VolumePass},
    volume_plan::{DepthSupport, VolumePlan, plan},
    volume_samples::volume_samples,
    volumes::scene_count,
};

// An image of a swapchain that wgpu draws into, with its view.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

// The swapchains of one eye. The images wrap the swapchains' own, so they drop first.
struct EyeSwapchains {
    colors: Vec<Target>,
    depths: Vec<Target>,
    color: xr::Swapchain<Api>,
    depth: Option<xr::Swapchain<Api>>,
    // The size of the eye's pictures in pixels
    size: (u32, u32),
}

// What a frame draws into the projection layer: the eyes' views, the size of their pictures in
// pixels, and the pictures of the layers that it draws in place of their own, which sample `atlas`.
pub(crate) struct ProjectionFrame<'a> {
    pub(crate) views: [Option<RenderViewpoint>; 2],
    pub(crate) size: (u32, u32),
    pub(crate) layers: &'a [ProjectedLayer],
    pub(crate) atlas: Option<&'a wgpu::Texture>,
}

// What the frame drew: the eyes' cameras, and the poses they were drawn from.
struct Drawn {
    cameras: [EyeCamera; 2],
    poses: [Pose; 2],
    plan: VolumePlan,
}

pub(crate) struct VolumeLayers {
    pass: VolumePass,
    color_format: SwapchainFormat,
    // The format of the depth swapchain, if the runtime offers one that the scenes draw against
    depth_format: Option<SwapchainFormat>,
    // Whether the instance has the extension that lets a layer carry depth, and the one that lets
    // the layers over it test against it
    extensions: (bool, bool),
    eyes: Option<[EyeSwapchains; 2]>,
    // How long the swapchains and the scene's window stay with nothing to draw
    hold: SceneHold,
    drawn: Option<Drawn>,
    // Whether the scene's samples are set: the first frame with a volume sets them
    sampled: bool,
    // The plan that the log named last, so that it names each change once
    logged: Option<VolumePlan>,
    // Whether the 3D content failed. A failure ends the 3D content and leaves the session, with
    // its panels, running.
    failed: bool,
    // The pass that draws the pictures of other layers, from the first frame that has one
    projected: Option<ProjectedPass>,
}

impl VolumeLayers {
    pub(crate) fn new(xr_session: &XrSession, color_format: SwapchainFormat) -> Self {
        let depth_format = SwapchainFormat::choose_depth(&xr_session.session);
        Self {
            pass: VolumePass::default(),
            color_format,
            depth_format,
            extensions: xr_session.depth_extensions(),
            eyes: None,
            hold: SceneHold::default(),
            drawn: None,
            sampled: false,
            logged: None,
            failed: false,
            projected: None,
        }
    }

    // What the runtime and the session offer for the depth of a projection layer.
    fn support(&self) -> DepthSupport {
        DepthSupport {
            submits: self.extensions.0 && self.depth_format.is_some(),
            tests: self.extensions.1,
        }
    }

    // Whether the frame can have the layer. A failed 3D content takes it away for good.
    pub(crate) fn available(&self) -> bool {
        !self.failed
    }

    // The window root that holds the scene of the volumes, once a draw found it.
    pub(crate) fn scene_root(&self) -> Option<cranpose_core::NodeId> {
        self.pass.root()
    }

    // What the last draw did, which tells how the frame's other layers stand to the depth of this
    // one. `None` for a frame with no projection layer.
    pub(crate) fn plan(&self) -> Option<VolumePlan> {
        self.drawn.as_ref().map(|drawn| drawn.plan)
    }

    // Draws the scene of the volumes and the pictures of `frame` once for each eye, if the app has
    // a volume or the frame has a picture and the runtime located both eyes. A frame with neither
    // makes no layer. Such frames free the swapchains and the scene's window once the hold ends. A
    // call of the runtime or of the renderer that fails ends the 3D content, with one line in the
    // log, and leaves the panels running.
    pub(crate) fn draw(
        &mut self,
        guest: &mut GuestView,
        gpu: &GpuContext,
        xr_session: &XrSession,
        space: &Space,
        frame: &ProjectionFrame<'_>,
    ) {
        if self.failed {
            return;
        }
        if let Err(error) = self.try_draw(guest, gpu, xr_session, space, frame) {
            log::error!("cranpose xr: the 3D content ends: {error}");
            self.failed = true;
            self.eyes = None;
            self.drawn = None;
            self.pass.release(guest);
            space.volumes().held().set(false);
        }
    }

    fn try_draw(
        &mut self,
        guest: &mut GuestView,
        gpu: &GpuContext,
        xr_session: &XrSession,
        space: &Space,
        frame: &ProjectionFrame<'_>,
    ) -> Result<(), String> {
        self.drawn = None;
        let ProjectionFrame {
            views,
            size,
            layers,
            atlas,
        } = *frame;
        // The swapchains stay through a frame that cannot draw, such as one in which the runtime
        // loses the eyes. They go with the scene's window when the hold after the last volume, the
        // last controller model and the last picture of another layer ends.
        let contents = scene_count(space) + layers.len();
        let held = space.volumes().held();
        if self.hold.frame(contents, Instant::now()) {
            self.eyes = None;
            self.pass.release(guest);
            held.set(false);
        } else if contents > 0 && !held.get_non_reactive() {
            held.set(true);
        }
        // A controller's model and the picture of another layer leave depth, and so may a volume
        let writes = space.volumes().writes_depth()
            || space.volumes().controllers().shown_count() > 0
            || !layers.is_empty();
        let depth = self.support().for_content(writes);
        let Some(frame_plan) = plan(contents, views.map(|view| view.is_some()), depth) else {
            return Ok(());
        };
        let Some((cameras, poses)) = eye_cameras(views, size) else {
            return Ok(());
        };
        self.sample_once(gpu, space);
        if self.eyes.is_none() {
            self.eyes = Some(self.make_swapchains(gpu, xr_session, size)?);
        }
        self.prepare_projected(gpu, layers, atlas);
        let Self {
            pass,
            eyes: Some(eyes),
            logged,
            projected,
            ..
        } = self
        else {
            return Ok(());
        };
        let projected = projected.as_ref().filter(|_| !layers.is_empty());
        // A swapchain that the runtime refused to make for the depth leaves its eye without depth,
        // and the layers over the projection then have none to test against
        let has_depth = eyes.iter().all(|eye| eye.depth.is_some());
        let frame_plan = VolumePlan {
            submits_depth: frame_plan.submits_depth && has_depth,
            tests_depth: frame_plan.tests_depth && has_depth,
        };
        if *logged != Some(frame_plan) {
            *logged = Some(frame_plan);
            log::info!(
                "cranpose xr: the volumes' layer carries depth {}, and the layers over it test it {}",
                frame_plan.submits_depth,
                frame_plan.tests_depth
            );
        }
        let mut eye_pass = EyePass {
            scene: pass,
            guest,
            gpu,
            space,
            projected: projected.map(|projected| (projected, layers)),
            submits_depth: frame_plan.submits_depth,
        };
        let mut drew = true;
        for (index, (eye, camera)) in eyes.iter_mut().zip(&cameras).enumerate() {
            drew &= eye_pass.draw(eye, (index, camera))?;
        }
        if drew {
            self.drawn = Some(Drawn {
                cameras,
                poses,
                plan: frame_plan,
            });
        }
        Ok(())
    }

    // Sets the samples of the scene of the volumes, once: at the first frame that draws.
    fn sample_once(&mut self, gpu: &GpuContext, space: &Space) {
        if self.sampled {
            return;
        }
        let samples = volume_samples(
            |format| {
                gpu.adapter
                    .get_texture_format_features(format)
                    .flags
                    .sample_count_supported(4)
            },
            self.color_format.wgpu,
        );
        log::info!(
            "cranpose xr: the volumes' scene takes {} samples",
            samples.samples()
        );
        space.volumes().samples().set(samples);
        self.sampled = true;
    }

    // Hands the pass that draws the pictures of other layers the frame's `layers`, which sample
    // `atlas`. The first frame that has any makes the pass.
    fn prepare_projected(
        &mut self,
        gpu: &GpuContext,
        layers: &[ProjectedLayer],
        atlas: Option<&wgpu::Texture>,
    ) {
        if layers.is_empty() {
            return;
        }
        self.projected
            .get_or_insert_with(|| {
                ProjectedPass::new(
                    gpu,
                    self.color_format.wgpu,
                    self.depth_format.map(|format| format.wgpu),
                    |kind| handle_layers::picture(kind, FirstRow::Top),
                )
            })
            .prepare(gpu, layers, atlas);
    }

    // The layer of the pictures that the last draw made, or `None` for a frame with no 3D content.
    pub(crate) fn layer(&self) -> Option<ProjectionLayer<'_>> {
        let (drawn, eyes) = (self.drawn.as_ref()?, self.eyes.as_ref()?);
        let eye = |index: usize| {
            let swapchains = &eyes[index];
            let camera = &drawn.cameras[index];
            ProjectionEye {
                swapchain: &swapchains.color,
                size: swapchains.size,
                pose: drawn.poses[index],
                fov: camera.field_of_view,
                depth: swapchains
                    .depth
                    .as_ref()
                    .filter(|_| drawn.plan.submits_depth)
                    .map(|swapchain| EyeDepth {
                        swapchain,
                        near: camera.depth_near,
                    }),
            }
        };
        Some(ProjectionLayer {
            eyes: [eye(0), eye(1)],
            alpha: Alpha::Premultiplied,
        })
    }

    // The swapchains of both eyes, with the depth swapchains where the runtime takes depth.
    fn make_swapchains(
        &self,
        gpu: &GpuContext,
        xr_session: &XrSession,
        size: (u32, u32),
    ) -> Result<[EyeSwapchains; 2], String> {
        let depth_format = self.depth_format.filter(|_| self.support().submits);
        let make = || -> Result<EyeSwapchains, String> {
            let (color, colors) = make_swapchain(
                xr_session,
                gpu,
                self.color_format,
                size,
                SwapchainUse::Color,
            )?;
            // The depth is an extra: a runtime that refuses its swapchain still shows the picture
            let depth = depth_format.and_then(|format| {
                make_swapchain(xr_session, gpu, format, size, SwapchainUse::Depth)
                    .inspect_err(|error| {
                        log::warn!("cranpose xr: no depth for the volumes: {error}");
                    })
                    .ok()
            });
            let (depth, depths) = match depth {
                Some((swapchain, targets)) => (Some(swapchain), targets),
                None => (None, Vec::new()),
            };
            Ok(EyeSwapchains {
                colors,
                depths,
                color,
                depth,
                size,
            })
        };
        Ok([make()?, make()?])
    }
}

// The cameras of both eyes for pictures of `size` pixels, with the poses that they draw from, or
// `None` while the runtime has not located both eyes.
fn eye_cameras(
    views: [Option<RenderViewpoint>; 2],
    size: (u32, u32),
) -> Option<([EyeCamera; 2], [Pose; 2])> {
    let first_row = if Graphics::TOP_ROW_FIRST {
        FirstRow::Top
    } else {
        FirstRow::Bottom
    };
    let [Some(left), Some(right)] = views else {
        return None;
    };
    Some((
        [
            EyeCamera::of(&left, size, first_row)?,
            EyeCamera::of(&right, size, first_row)?,
        ],
        [left.pose, right.pose],
    ))
}

// What draws into each eye's image: the scene of the volumes and the pictures of other layers.
struct EyePass<'a> {
    scene: &'a mut VolumePass,
    guest: &'a mut GuestView,
    gpu: &'a GpuContext,
    space: &'a Space,
    projected: Option<(&'a ProjectedPass, &'a [ProjectedLayer])>,
    submits_depth: bool,
}

impl EyePass<'_> {
    // Draws the next image of `eye`, the eye at `index` that `camera` sees. Returns whether the
    // image holds a picture.
    fn draw(
        &mut self,
        eye: &mut EyeSwapchains,
        (index, camera): (usize, &EyeCamera),
    ) -> Result<bool, String> {
        let color_index = acquire(&mut eye.color)?;
        let depth_index = match (&mut eye.depth, self.submits_depth) {
            (Some(depth), true) => Some(acquire(depth)?),
            _ => None,
        };
        let color = &eye.colors[color_index];
        let depth = depth_index.map(|index| &eye.depths[index]);
        let drew = self.draw_images(color, depth, eye.size, (index, camera));
        // The images go back whether or not the scene drew, so the swapchain stays in step
        eye.color
            .release_image()
            .map_err(|error| format!("OpenXR release: {error}"))?;
        if let (Some(depth), Some(_)) = (&mut eye.depth, depth_index) {
            depth
                .release_image()
                .map_err(|error| format!("OpenXR release depth: {error}"))?;
        }
        drew
    }

    // Draws the scene and the pictures of other layers into the eye's `color` and `depth`, images
    // of `size` pixels. Returns whether the images hold a picture.
    fn draw_images(
        &mut self,
        color: &Target,
        depth: Option<&Target>,
        size: (u32, u32),
        (index, camera): (usize, &EyeCamera),
    ) -> Result<bool, String> {
        let drew = self.scene.draw(
            self.guest,
            self.space,
            camera,
            &EyeImage {
                color: &color.texture,
                color_view: &color.view,
                depth: depth.map(|target| &target.view),
            },
        )?;
        if !(Graphics::LAYOUT_BARRIERS && drew) && self.projected.is_none() {
            return Ok(drew);
        }
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cranpose xr volumes"),
            });
        if let Some((pass, layers)) = self.projected {
            let target = EyeTarget {
                color: &color.view,
                depth: depth.map(|target| &target.view),
                size,
                drawn: drew,
            };
            pass.draw(self.gpu, &mut encoder, (index, camera), &target, layers);
        }
        if Graphics::LAYOUT_BARRIERS {
            Graphics::ready_for_release(&mut encoder, &color.texture);
            if let Some(depth) = depth {
                Graphics::ready_for_release(&mut encoder, &depth.texture);
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        Ok(drew || self.projected.is_some())
    }
}

// The index of the swapchain's next image, once the compositor has finished with it.
fn acquire(swapchain: &mut xr::Swapchain<Api>) -> Result<usize, String> {
    let index = swapchain
        .acquire_image()
        .map_err(|error| format!("OpenXR acquire: {error}"))?;
    swapchain
        .wait_image(xr::Duration::INFINITE)
        .map_err(|error| format!("OpenXR wait image: {error}"))?;
    Ok(index as usize)
}

// What a swapchain holds.
#[derive(Clone, Copy)]
enum SwapchainUse {
    Color,
    Depth,
}

// Makes a swapchain of `size` pixels in `format` and wraps each of its images as a texture that
// wgpu draws into. A color image is a render attachment and nothing else, so the renderer draws
// the scene into a target of its own and converts it into the image, as it does for the panels'
// atlas. It draws straight into an image only when the image also takes copies, and then an sRGB
// image would need a view of another format, which a swapchain's image does not offer.
fn make_swapchain(
    xr_session: &XrSession,
    gpu: &GpuContext,
    format: SwapchainFormat,
    size: (u32, u32),
    kind: SwapchainUse,
) -> Result<(xr::Swapchain<Api>, Vec<Target>), String> {
    let (swapchain_usage, texture_usage) = match kind {
        SwapchainUse::Color => (
            xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::SAMPLED
                | xr::SwapchainUsageFlags::TRANSFER_SRC
                | xr::SwapchainUsageFlags::TRANSFER_DST,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ),
        SwapchainUse::Depth => (
            xr::SwapchainUsageFlags::DEPTH_STENCIL_ATTACHMENT | xr::SwapchainUsageFlags::SAMPLED,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ),
    };
    let swapchain = xr_session
        .session
        .create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: swapchain_usage,
            format: format.api,
            sample_count: 1,
            width: size.0,
            height: size.1,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })
        .map_err(|error| format!("OpenXR volume swapchain: {error}"))?;
    let targets = swapchain
        .enumerate_images()
        .map_err(|error| format!("OpenXR volume swapchain images: {error}"))?
        .into_iter()
        .map(|image| {
            let texture =
                Graphics::wrap_image(&gpu.device, image, size, format.wgpu, texture_usage)?;
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            Ok(Target { texture, view })
        })
        .collect::<Result<_, String>>()?;
    Ok((swapchain, targets))
}
