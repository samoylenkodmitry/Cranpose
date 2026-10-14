//! `AsyncImage`: an image that loads from a URL, a file or bytes, as Coil's.

use cranpose_services::{ImageLoadError, ImageLoadState, ImageRequest, rememberAsyncImage};
use cranpose_ui::{
    Alignment, BitmapPainter, BoxWithConstraints, BoxWithConstraintsScope, ContentScale, Image,
    Modifier, composable, density,
};

/// Device pixels for `logical` pixels of the layout; no limit for an
/// unbounded side.
fn device_pixels(logical: f32, scale: f32) -> u32 {
    if logical.is_finite() {
        (logical * scale).ceil().clamp(1.0, u32::MAX as f32) as u32
    } else {
        u32::MAX
    }
}

/// Shows the image `request` loads in the box the layout gives it, as Coil's
/// `AsyncImage`: `placeholder` while it loads, `error` when it fails, then
/// the image, scaled into the box by `content_scale`.
///
/// A request without a size of its own decodes no larger than the box in
/// device pixels, so a large photo in a thumbnail holds a thumbnail's
/// memory. Give the image a size, or a box with one, for that: in an
/// unbounded side it decodes at full size.
#[composable]
pub fn AsyncImage(
    request: ImageRequest,
    content_description: Option<String>,
    modifier: Modifier,
    content_scale: ContentScale,
    placeholder: impl Fn() + 'static,
    error: impl Fn(&ImageLoadError) + 'static,
) {
    BoxWithConstraints(modifier, move |scope| {
        let constraints = scope.constraints();
        let scale = density().density();
        let request = request.clone().or_max_size(
            device_pixels(constraints.max_width, scale),
            device_pixels(constraints.max_height, scale),
        );
        match rememberAsyncImage(request).value() {
            ImageLoadState::Ready(bitmap) => {
                let bounded =
                    constraints.max_width.is_finite() && constraints.max_height.is_finite();
                let fill = if bounded {
                    Modifier::empty().fill_max_size()
                } else {
                    Modifier::empty()
                };
                Image(
                    BitmapPainter(bitmap),
                    content_description.clone(),
                    fill,
                    Alignment::CENTER,
                    content_scale,
                    1.0,
                    None,
                );
            }
            ImageLoadState::Loading => placeholder(),
            ImageLoadState::Failed(failure) => error(&failure),
        }
    });
}
