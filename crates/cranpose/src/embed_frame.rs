use crate::embed_protocol::PixelRect;
pub(crate) use crate::frame_readback::FrameTarget;
pub(crate) const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

const BYTES_PER_PIXEL: u32 = 4;

pub(crate) fn changed_rect(
    previous: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Option<PixelRect> {
    let row_bytes = (width * BYTES_PER_PIXEL) as usize;
    if row_bytes == 0 || previous.len() != next.len() {
        return Some(PixelRect::full(width, height));
    }
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    let rows = previous
        .chunks_exact(row_bytes)
        .zip(next.chunks_exact(row_bytes));
    for (row, (before, after)) in rows.enumerate() {
        if before == after {
            continue;
        }
        let Some((first, last)) = changed_columns(before, after) else {
            continue;
        };
        let row = row as u32;
        bounds = Some(match bounds {
            None => (first, last, row, row),
            Some((first_column, last_column, first_row, _)) => (
                first_column.min(first),
                last_column.max(last),
                first_row,
                row,
            ),
        });
    }
    bounds.map(
        |(first_column, last_column, first_row, last_row)| PixelRect {
            x: first_column,
            y: first_row,
            width: last_column - first_column + 1,
            height: last_row - first_row + 1,
        },
    )
}

fn changed_columns(before: &[u8], after: &[u8]) -> Option<(u32, u32)> {
    let pixel_size = BYTES_PER_PIXEL as usize;
    let pixels = || {
        before
            .chunks_exact(pixel_size)
            .zip(after.chunks_exact(pixel_size))
    };
    let first = pixels().position(|(a, b)| a != b)?;
    let last = pixels().rposition(|(a, b)| a != b)?;
    Some((first as u32, last as u32))
}

#[cfg(test)]
#[path = "tests/embed_frame_tests.rs"]
mod tests;
