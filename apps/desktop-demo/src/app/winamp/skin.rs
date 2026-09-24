use std::{
    collections::HashMap,
    io::{Cursor, Read},
};

use anyhow::{Context, Result};
use cranpose_ui::ImageBitmap;

#[derive(Clone, PartialEq)]
pub struct WinampSkin {
    pub main: ImageBitmap,
    pub titlebar: ImageBitmap,
    pub cbuttons: ImageBitmap,
    pub posbar: ImageBitmap,
    pub shufrep: ImageBitmap,
    pub volume: ImageBitmap,
    pub balance: ImageBitmap,
    pub playpaus: ImageBitmap,
    pub monoster: ImageBitmap,
    pub numbers: ImageBitmap,
    pub eqmain: ImageBitmap,
    pub pledit: ImageBitmap,
}

pub fn load_skin(wsz_bytes: &[u8]) -> Result<WinampSkin> {
    let mut archive = zip::ZipArchive::new(Cursor::new(wsz_bytes))
        .context("failed to open winamp .wsz archive")?;

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    for idx in 0..archive.len() {
        let mut file = archive.by_index(idx).context("failed to read zip entry")?;
        if file.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .with_context(|| format!("failed to read entry {}", file.name()))?;
        files.insert(normalize_name(file.name()), data);
    }

    let decode = |name: &str| -> Result<ImageBitmap> {
        let bytes = files
            .get(name)
            .with_context(|| format!("missing required skin entry: {name}"))?;
        decode_bmp(bytes).with_context(|| format!("failed to decode {name}"))
    };

    Ok(WinampSkin {
        main: decode("main.bmp")?,
        titlebar: decode("titlebar.bmp")?,
        cbuttons: decode("cbuttons.bmp")?,
        posbar: decode("posbar.bmp")?,
        shufrep: decode("shufrep.bmp")?,
        volume: decode("volume.bmp")?,
        balance: decode("balance.bmp")?,
        playpaus: decode("playpaus.bmp")?,
        monoster: decode("monoster.bmp")?,
        numbers: decode("numbers.bmp")?,
        eqmain: decode("eqmain.bmp")?,
        pledit: decode("pledit.bmp")?,
    })
}

fn normalize_name(name: &str) -> String {
    name.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase()
}

fn decode_bmp(bytes: &[u8]) -> Result<ImageBitmap> {
    let dynamic = image::load_from_memory(bytes).context("image decode")?;
    let mut rgba = dynamic.to_rgba8();

    for pixel in rgba.pixels_mut() {
        if pixel[0] == 255 && pixel[1] == 0 && pixel[2] == 255 {
            pixel[3] = 0;
        }
    }

    ImageBitmap::from_rgba8(rgba.width(), rgba.height(), rgba.into_raw())
        .context("failed to create image bitmap")
}

#[cfg(test)]
#[path = "tests/skin_tests.rs"]
mod tests;
