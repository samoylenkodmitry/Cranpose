//! In-memory pictures: RGBA pixels as the PNG file an `image` element names
//! by URL.

use zgui_image::ImageBytes;

/// The picture `width` by `height` pixels whose straight RGBA pixels are
/// `rgba`.
pub(crate) fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> ImageBytes {
    let mut file = Vec::new();
    let mut encoder = png::Encoder::new(&mut file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a PNG header");
    writer.write_image_data(rgba).expect("the picture's pixels");
    writer.finish().expect("a finished PNG");
    ImageBytes::new(file)
}
