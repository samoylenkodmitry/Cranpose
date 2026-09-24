use super::*;

fn pixel(bitmap: &ImageBitmap, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * bitmap.width() + x) * 4) as usize;
    let px = bitmap.pixels();
    [px[idx], px[idx + 1], px[idx + 2], px[idx + 3]]
}

#[test]
fn chessboard_bitmap_has_expected_dimensions() {
    let bitmap = generate_chessboard_bitmap(8, 4);
    assert_eq!(bitmap.width(), 32);
    assert_eq!(bitmap.height(), 32);
}

#[test]
fn chessboard_bitmap_alternates_colors() {
    let bitmap = generate_chessboard_bitmap(4, 2);
    assert_eq!(pixel(&bitmap, 1, 1), CHESSBOARD_LIGHT);
    assert_eq!(pixel(&bitmap, 5, 1), CHESSBOARD_DARK);
    assert_eq!(pixel(&bitmap, 1, 5), CHESSBOARD_DARK);
    assert_eq!(pixel(&bitmap, 5, 5), CHESSBOARD_LIGHT);
}
