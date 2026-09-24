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
#[path = "tests/net_image_tests.rs"]
mod tests;
