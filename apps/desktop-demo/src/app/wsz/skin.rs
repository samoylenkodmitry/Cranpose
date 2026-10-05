use std::io::{Cursor, Read};

use anyhow::{Context, Result};
use cranpose_ui::ImageBitmap;

#[derive(Clone, PartialEq)]
pub struct WszSkin {
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

pub fn load_skin(wsz_bytes: &[u8]) -> Result<WszSkin> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(wsz_bytes)).context("failed to open WSZ archive")?;
    let mut bytes = Vec::new();
    let mut decode = |name: &str| -> Result<ImageBitmap> {
        let index = archive
            .file_names()
            .position(|entry| {
                entry
                    .rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|basename| basename.trim().eq_ignore_ascii_case(name))
            })
            .with_context(|| format!("missing required skin entry: {name}"))?;
        let mut file = archive
            .by_index(index)
            .context("failed to read zip entry")?;
        bytes.clear();
        file.read_to_end(&mut bytes)
            .with_context(|| format!("failed to read entry {name}"))?;
        decode_bmp(&bytes).with_context(|| format!("failed to decode {name}"))
    };

    Ok(WszSkin {
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

fn decode_bmp(bytes: &[u8]) -> Result<ImageBitmap> {
    let dynamic = image::load_from_memory(bytes).context("image decode")?;
    let mut rgba = dynamic.into_rgba8();

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
