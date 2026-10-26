use super::*;

// The bytes a display reads for a pixel stored high byte first: red's five
// bits, green's six, blue's five.
#[test]
fn a_big_endian_pixel_stores_its_high_byte_first() {
    let pixel = Rgb565Be::from_rgb([0xF8, 0xFC, 0xF8, 255]);
    assert_eq!(bytemuck::bytes_of(&pixel), [0xFF, 0xFF]);
    let red = Rgb565Be::from_rgb([0xFF, 0x00, 0x00, 255]);
    assert_eq!(bytemuck::bytes_of(&red), [0xF8, 0x00]);
    let blue = Rgb565Le::from_rgb([0x00, 0x00, 0xFF, 255]);
    assert_eq!(bytemuck::bytes_of(&blue), [0x1F, 0x00]);
}

#[test]
fn a_pixel_widens_back_to_the_bytes_it_keeps() {
    for bytes in [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [27, 31, 51, 255],
        [247, 127, 0, 255],
    ] {
        let widened = Rgb565Be::from_rgb(bytes).to_rgba();
        assert_eq!(Rgb565Be::from_rgb(widened), Rgb565Be::from_rgb(bytes));
        assert_eq!(widened[0] >> 3, bytes[0] >> 3);
        assert_eq!(widened[1] >> 2, bytes[1] >> 2);
        assert_eq!(widened[2] >> 3, bytes[2] >> 3);
    }
}

#[test]
fn a_glyph_pixel_blends_as_its_bytes_would() {
    let mut pixel = Rgb565Le::from_rgb([0, 0, 0, 255]);
    pixel.blend_bytes([255, 255, 255, 128]);
    let mut bytes = [0, 0, 0, 255];
    bytes.blend_bytes([255, 255, 255, 128]);
    assert_eq!(pixel, Rgb565Le::from_rgb(bytes));
}
