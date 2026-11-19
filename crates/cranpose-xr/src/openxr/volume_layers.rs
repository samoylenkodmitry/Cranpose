//! The projection layer of the app's 3D content: a swapchain of pictures for each eye, and a second
//! one for the depth of those pictures when the runtime takes it.
//!
//! The composition draws the scene of the volumes into the eye's next image from that eye's own
//! pose and field of view, once for each eye in each frame. The scene draws against the depth
//! image, so the compositor receives the depth of what it shows. With no volume in the
//! composition no swapchain exists and no pass runs.

use cranpose::{GpuContext, GuestView};
use openxr as xr;

use super::{
    graphics::{Api, Graphics, SwapchainFormat},
    session::{Alpha, EyeDepth, ProjectionEye, ProjectionLayer, XrSession},
};
use crate::{
    Pose, RenderViewpoint,
    eye_camera::{EyeCamera, FirstRow},
    space::Space,
    volume_pass::{EyeImage, VolumePass},
    volume_plan::{DepthSupport, VolumePlan, plan},
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
    drawn: Option<Drawn>,
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
            drawn: None,
        }
    }

    // What the runtime and the session offer for the depth of a projection layer.
    fn support(&self) -> DepthSupport {
        DepthSupport {
            submits: self.extensions.0 && self.depth_format.is_some(),
            tests: self.extensions.1,
        }
    }

    // What the last draw did, which tells how the frame's other layers stand to the depth of this
    // one. `None` for a frame with no projection layer.
    pub(crate) fn plan(&self) -> Option<VolumePlan> {
        self.drawn.as_ref().map(|drawn| drawn.plan)
    }

    // Draws the scene of the volumes once for each eye, if the app has a volume and the runtime
    // located both eyes, into pictures of `size` pixels. A frame with no volume makes no layer
    // and frees the swapchains.
    pub(crate) fn draw(
        &mut self,
        guest: &mut GuestView,
        gpu: &GpuContext,
        xr_session: &XrSession,
        space: &Space,
        views: [Option<RenderViewpoint>; 2],
        size: (u32, u32),
    ) -> Result<(), String> {
        self.drawn = None;
        let Some(frame_plan) = plan(
            space.volume_count(),
            views.map(|view| view.is_some()),
            self.support(),
        ) else {
            self.eyes = None;
            return Ok(());
        };
        let first_row = if Graphics::TOP_ROW_FIRST {
            FirstRow::Top
        } else {
            FirstRow::Bottom
        };
        let [Some(left), Some(right)] = views else {
            return Ok(());
        };
        let (Some(left_camera), Some(right_camera)) = (
            EyeCamera::of(&left, size, first_row),
            EyeCamera::of(&right, size, first_row),
        ) else {
            return Ok(());
        };
        let cameras = [left_camera, right_camera];
        if self.eyes.is_none() {
            self.eyes = Some(self.make_swapchains(gpu, xr_session, size)?);
        }
        let Self {
            pass,
            eyes: Some(eyes),
            ..
        } = self
        else {
            return Ok(());
        };
        let mut drew = true;
        for (eye, camera) in eyes.iter_mut().zip(&cameras) {
            drew &= draw_eye(
                pass,
                guest,
                gpu,
                space,
                eye,
                camera,
                frame_plan.submits_depth,
            )?;
        }
        if drew {
            self.drawn = Some(Drawn {
                cameras,
                poses: [left.pose, right.pose],
                plan: frame_plan,
            });
        }
        Ok(())
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
                        near: camera.near,
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
            let depth = depth_format
                .map(|format| make_swapchain(xr_session, gpu, format, size, SwapchainUse::Depth))
                .transpose()?;
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

// Draws the next image of `eye`. Returns whether the scene drew.
fn draw_eye(
    pass: &mut VolumePass,
    guest: &mut GuestView,
    gpu: &GpuContext,
    space: &Space,
    eye: &mut EyeSwapchains,
    camera: &EyeCamera,
    submits_depth: bool,
) -> Result<bool, String> {
    let color_index = acquire(&mut eye.color)?;
    let depth_index = match (&mut eye.depth, submits_depth) {
        (Some(depth), true) => Some(acquire(depth)?),
        _ => None,
    };
    let color = &eye.colors[color_index];
    let depth = depth_index.map(|index| &eye.depths[index]);
    let drew = pass.draw(
        guest,
        space,
        camera,
        &EyeImage {
            color: &color.texture,
            color_view: &color.view,
            depth: depth.map(|target| &target.view),
        },
    );
    if Graphics::LAYOUT_BARRIERS && matches!(drew, Ok(true)) {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cranpose xr volumes"),
            });
        Graphics::ready_for_release(&mut encoder, &color.texture);
        if let Some(depth) = depth {
            Graphics::ready_for_release(&mut encoder, &depth.texture);
        }
        gpu.queue.submit([encoder.finish()]);
    }
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
