//! `PlayerSurfaceIntake`: a player surface whose app takes the pictures from the decoder.

use cranpose_core::{LaunchedEffect, MutableState, rememberMutableStateOf};
use cranpose_services::{CropRect, DecodedFrames, VideoFrame};
use cranpose_ui::{
    BitmapRegionPainter, ContentScale, ImageSampling, Modifier, Painter, Rect, composable,
};

use self::platform::{Converter, Gpu, SUPPORTED, converter_of, gpu};
use crate::player_surface::Picture;

/// Whether this build can import decoded [`VideoFrame`]s directly onto the GPU for
/// [`PlayerSurface`](crate::PlayerSurface) and [`PlayerSurfaceIntake`].
///
/// Returns `true` on GPU-backed targets with a zero-copy video importer and `false` on CPU
/// framebuffer targets (such as watchOS and `renderer-pixels`) or platforms without a GPU video
/// importer.
pub const fn video_surface_supported() -> bool {
    SUPPORTED
}

/// Shows in the box that the layout gives it the pictures that `intake` presents. The app decides
/// what happens to each picture that the decoder outputs: which one is shown, which part of it,
/// and which pictures are counted as dropped. [`PlayerSurface`](crate::PlayerSurface) is this
/// composable with an intake that presents the newest picture.
///
/// `intake` runs on the thread of the composition each time `frames` holds pictures. It takes the
/// pictures that it wants with [`next_frame`](DecodedFrames::next_frame), and gives the one to
/// show to [`FramePresenter::present`]. A picture that `intake` leaves in `frames`, and a picture
/// that the decoder outputs while `intake` runs, wait for the next call. The surface makes that
/// call at the window's next frame, so `intake` runs at most once a frame. The surface calls `intake` also where it cannot draw, so that the
/// app drains the decoder and counts what it could not show. The surface keeps the `intake` of
/// the composition in which `frames` last changed.
///
/// The surface shows the last presented picture again while no newer one comes, scaled by
/// `content_scale`. It waits for the decoder: no work is done in a frame that has no new picture.
/// The platforms, the converter and the decoder's `depth` are those of
/// [`PlayerSurface`](crate::PlayerSurface). A box with no picture draws nothing and keeps its
/// size.
#[composable]
pub fn PlayerSurfaceIntake(
    frames: DecodedFrames,
    modifier: Modifier,
    content_scale: ContentScale,
    intake: impl FnMut(&DecodedFrames, &mut FramePresenter) + 'static,
) {
    let shown = rememberMutableStateOf(|| None::<Painter>);
    pump_with(frames, shown, intake);
    Picture(shown, modifier, content_scale);
}

/// The way from a decoded picture to the box of a [`PlayerSurfaceIntake`].
pub struct FramePresenter {
    shown: MutableState<Option<Painter>>,
    converter: Option<Converter>,
}

impl FramePresenter {
    /// Converts `frame` on the GPU and shows it, or only `part` of it. `part` is in pixels from
    /// the picture's top left corner, which is the corner of [`VideoFrame::crop`]. It scales as
    /// the whole picture does: the box shows `part` at the size that `content_scale` gives it.
    ///
    /// Returns an error, and keeps the picture that was shown, when the frame is not shown. The
    /// frame drops in each case.
    pub fn present(
        &mut self,
        frame: VideoFrame,
        part: Option<CropRect>,
    ) -> Result<(), PresentError> {
        let display_size = frame.display_size();
        let region = match part {
            Some(part) if !inside(part, display_size) => {
                return Err(PresentError::PartOutside);
            }
            Some(part) => Some(Rect {
                x: part.x as f32,
                y: part.y as f32,
                width: part.width as f32,
                height: part.height as f32,
            }),
            None => None,
        };
        let Some(bitmap) = self.converter.as_mut().and_then(|c| c.convert(frame)) else {
            return Err(PresentError::NotImported);
        };
        let painter = match region {
            Some(region) => BitmapRegionPainter(bitmap, region, ImageSampling::Linear),
            None => Painter::from_bitmap(bitmap),
        };
        self.shown.set(Some(painter));
        Ok(())
    }
}

/// Why [`FramePresenter::present`] did not show a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PresentError {
    /// The part to show is empty or reaches outside the picture.
    #[error("the part to show is empty or reaches outside the picture")]
    PartOutside,
    /// The platform or the renderer cannot import the picture, or the GPU holds too many
    /// pictures that the window has not drawn.
    #[error("the picture could not be imported for drawing")]
    NotImported,
}

// Whether `part` is not empty and lies in a picture of `picture` pixels.
fn inside(part: CropRect, picture: (u32, u32)) -> bool {
    part.width > 0
        && part.height > 0
        && part
            .x
            .checked_add(part.width)
            .is_some_and(|right| right <= picture.0)
        && part
            .y
            .checked_add(part.height)
            .is_some_and(|bottom| bottom <= picture.1)
}

// Calls `intake` each time the decoder has pictures, with the converter of the renderer's GPU.
#[composable]
pub(crate) fn pump_with(
    frames: DecodedFrames,
    shown: MutableState<Option<Painter>>,
    intake: impl FnMut(&DecodedFrames, &mut FramePresenter) + 'static,
) {
    // A lost device gives a new context, which makes a new converter
    let gpu: Gpu = gpu();
    LaunchedEffect((frames.clone(), gpu.clone()), async move |scope| {
        // A painter of the old context's textures does not draw on the new device
        shown.set(None);
        let mut presenter = FramePresenter {
            shown,
            converter: converter_of(&gpu),
        };
        let mut intake = intake;
        let clock = scope.runtime().frame_clock();
        loop {
            frames.wait_for_frame().await;
            intake(&frames, &mut presenter);
            // A picture that waits ends the next wait at once: one that the intake left, or one
            // that came while it ran. The next call waits for the window's next frame, so the
            // loop yields to the window between two calls.
            clock.next_frame().await;
        }
    });
}

// The converter of the platform's decoded pictures: one pass that turns a picture into RGB
#[cfg(cranpose_gpu_video)]
mod platform {
    #[cfg(all(feature = "android", target_os = "android"))]
    pub(super) use crate::android::ycbcr_converter::YcbcrConverter as Converter;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) use crate::apple::nv12_converter::Nv12Converter as Converter;
    #[cfg(target_os = "linux")]
    pub(super) use crate::linux::video_converter::LinuxVideoConverter as Converter;
    #[cfg(all(feature = "web", target_arch = "wasm32", target_os = "unknown"))]
    pub(super) use crate::web::video_converter::WebVideoConverter as Converter;
    #[cfg(target_os = "windows")]
    pub(super) use crate::windows::d3d12_converter::D3d12Converter as Converter;
    use crate::{GpuContext, gpu_context};

    pub(super) const SUPPORTED: bool = true;

    pub(super) type Gpu = Option<GpuContext>;

    pub(super) fn gpu() -> Gpu {
        gpu_context()
    }

    pub(super) fn converter_of(gpu: &Gpu) -> Option<Converter> {
        gpu.as_ref().and_then(Converter::new)
    }
}

// A build that cannot import a decoder's pictures converts none, so every picture drops.
#[cfg(not(cranpose_gpu_video))]
mod platform {
    use cranpose_services::VideoFrame;
    use cranpose_ui::ImageBitmap;

    pub(super) const SUPPORTED: bool = false;

    pub(super) struct Converter;

    impl Converter {
        pub(super) fn convert(&mut self, _frame: VideoFrame) -> Option<ImageBitmap> {
            None
        }
    }

    // The key of the effect, which no renderer context changes
    #[derive(Clone, PartialEq)]
    pub(super) struct Gpu;

    pub(super) fn gpu() -> Gpu {
        Gpu
    }

    pub(super) fn converter_of(_gpu: &Gpu) -> Option<Converter> {
        None
    }
}
