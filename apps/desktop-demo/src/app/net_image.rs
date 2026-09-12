use anyhow::{anyhow, Context};
use cranpose_ui::ImageBitmap;

pub(crate) fn cors_url(url: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        format!(
            "https://cranpose-cors-proxy.cranpose.workers.dev/?url={}",
            url_encode(url)
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        url.to_string()
    }
}

#[cfg(target_arch = "wasm32")]
fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    encoded
}

pub(crate) fn decode_bitmap(bytes: &[u8]) -> anyhow::Result<ImageBitmap> {
    let image = image::load_from_memory(bytes).context("failed to decode image bytes")?;
    let rgba = image.to_rgba8();
    ImageBitmap::from_rgba8(image.width(), image.height(), rgba.into_raw())
        .map_err(|err| anyhow!("invalid RGBA bitmap: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_bytes_that_are_not_an_image() {
        assert!(decode_bitmap(b"not an image").is_err());
    }

    #[test]
    fn decode_accepts_a_one_pixel_png() {
        let png: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let bitmap = decode_bitmap(png).expect("one-pixel png decodes");
        assert_eq!((bitmap.width(), bitmap.height()), (1, 1));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_requests_go_straight_to_the_origin() {
        assert_eq!(
            cors_url("https://example.com/a.png"),
            "https://example.com/a.png"
        );
    }
}
