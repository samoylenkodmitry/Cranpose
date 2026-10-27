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
            *dst = [
                mix_byte(src[0], dst[0], alpha),
                mix_byte(src[1], dst[1], alpha),
                mix_byte(src[2], dst[2], alpha),
                mix_byte(255, dst[3], alpha),
            ];
        }
    }
}

// One channel of `blend_byte_pixel`: `source` at `alpha` over `under`.
pub(crate) fn mix_byte(source: u8, under: u8, alpha: u32) -> u8 {
    ((u32::from(source) * alpha + u32::from(under) * (255 - alpha) + 127) / 255) as u8
}
