/// Draws a straight-alpha byte pixel over `dst`, source over, with the bytes
/// a float blend would work out. Each channel is the rational
/// `(s * a + d * (255 - a)) / 255` rounded: its fraction is a multiple of
/// 1/255, never one half, and the float error stays far below that gap, so
/// whole numbers round it the same way.
pub fn blend_byte_pixel(dst: &mut [u8; 4], src: [u8; 4]) {
    let alpha = u32::from(src[3]);
    match alpha {
        0 => {}
        255 => *dst = src,
        _ => {
            let keep = 255 - alpha;
            let mix = |source: u8, under: u8| {
                ((u32::from(source) * alpha + u32::from(under) * keep + 127) / 255) as u8
            };
            *dst = [
                mix(src[0], dst[0]),
                mix(src[1], dst[1]),
                mix(src[2], dst[2]),
                mix(255, dst[3]),
            ];
        }
    }
}
