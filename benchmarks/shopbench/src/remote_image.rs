// An image from the API, cropped to fill its frame, on the `surface` color
// until it has loaded.

use cranpose::{AsyncImage, Box, BoxSpec, ContentScale, ImageRequest, Modifier, composable};

use crate::theme::SURFACE;

#[composable]
pub(crate) fn RemoteImage(url: Option<String>, modifier: Modifier) {
    let modifier = modifier.background(SURFACE).clip_to_bounds();
    match url {
        Some(url) => {
            AsyncImage(
                ImageRequest::new(url),
                None,
                modifier,
                ContentScale::Crop,
                || {},
                |_| {},
            );
        }
        None => {
            Box(modifier, BoxSpec::default(), || {});
        }
    }
}
