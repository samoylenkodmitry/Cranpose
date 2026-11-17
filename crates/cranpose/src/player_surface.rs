//! `PlayerSurface`: the pictures of a hardware video decoder, drawn in the layout.

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_services::DecodedFrames;
use cranpose_ui::{
    Alignment, Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, ContentScale, Image,
    ImageBitmap, Modifier, composable,
};

/// Shows the pictures of a [`VideoDecoder`](crate::VideoDecoder) in the box that the layout gives
/// it, scaled by `content_scale`, as Media3's `PlayerSurface` shows a player's video. `frames`
/// are the pictures of a decoder whose output is
/// [`DecoderOutput::Frames`](crate::DecoderOutput::Frames).
///
/// Media3's `PlayerSurface` takes a `Player`, and Cranpose has none: the pictures come from the
/// decoder that the app feeds. The `content_scale` is Media3's `ContentFrame` parameter.
///
/// The surface draws the newest picture in the frame that follows its output, and shows the last
/// picture again while no newer one comes. It drops the pictures that it did not show. It waits
/// for the decoder: no work is done in a frame that has no new picture. A picture is converted
/// once on the GPU, with no copy through the CPU, and the decoder's own buffer is the source.
/// The surface keeps each picture until the GPU has read it, which takes up to three frames, so
/// give the decoder a `depth` of 6 or more for a stream of 60 pictures a second. The decoder
/// drops its oldest pictures first when the app falls behind, as it does while a window starts.
///
/// Platforms: macOS and iOS draw through Metal. Android draws through Vulkan: the app's device
/// is opened with the extensions that import a hardware buffer, which the GPU's driver offers on
/// Android 8 and later. Where the device lacks them, or where Android renders with OpenGL ES, the
/// surface draws nothing. No other platform has a hardware decoder that Cranpose drives. A box
/// with no picture draws nothing and keeps its size.
#[composable]
pub fn PlayerSurface(frames: DecodedFrames, modifier: Modifier, content_scale: ContentScale) {
    let shown = rememberMutableStateOf(|| None::<ImageBitmap>);
    pump(frames, shown);
    BoxWithConstraints(modifier, move |scope| {
        let constraints = scope.constraints();
        let bounded = constraints.max_width.is_finite() && constraints.max_height.is_finite();
        let fill = if bounded {
            Modifier::empty().fill_max_size()
        } else {
            Modifier::empty()
        };
        if let Some(bitmap) = shown.value() {
            Image(
                bitmap,
                None,
                fill,
                Alignment::CENTER,
                content_scale,
                1.0,
                None,
            );
        } else {
            Box(fill, BoxSpec::default(), || {});
        }
    });
}

// Takes each picture that the decoder outputs and puts its bitmap in `shown`.
#[cfg(all(
    feature = "renderer-wgpu",
    any(
        target_os = "macos",
        target_os = "ios",
        all(feature = "android", target_os = "android")
    )
))]
#[composable]
fn pump(frames: DecodedFrames, shown: MutableState<Option<ImageBitmap>>) {
    use cranpose_core::LaunchedEffect;

    use crate::gpu_context;

    // A lost device gives a new context, which makes a new converter
    let gpu = gpu_context();
    LaunchedEffect((frames.clone(), gpu.clone()), async move |_scope| {
        // A bitmap of the old context's textures does not draw on the new device
        shown.set(None);
        let Some(gpu) = gpu else {
            return;
        };
        let Some(mut converter) = Converter::new(&gpu) else {
            return;
        };
        loop {
            frames.wait_for_frame().await;
            if let Some(frame) = frames.latest_frame()
                && let Some(bitmap) = converter.convert(frame)
            {
                shown.set(Some(bitmap));
            }
        }
    });
}

// The converter of the platform's decoded pictures: one pass that turns a picture into RGB
#[cfg(all(feature = "renderer-wgpu", feature = "android", target_os = "android"))]
use crate::android::ycbcr_converter::YcbcrConverter as Converter;
#[cfg(all(feature = "renderer-wgpu", any(target_os = "macos", target_os = "ios")))]
use crate::apple::nv12_converter::Nv12Converter as Converter;

#[cfg(not(all(
    feature = "renderer-wgpu",
    any(
        target_os = "macos",
        target_os = "ios",
        all(feature = "android", target_os = "android")
    )
)))]
#[composable]
fn pump(_frames: DecodedFrames, _shown: MutableState<Option<ImageBitmap>>) {}
