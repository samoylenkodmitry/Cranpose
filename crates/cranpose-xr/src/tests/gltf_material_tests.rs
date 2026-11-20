// The picture of a material: the base color factor multiplied into the texture, the alpha mode,
// and a texture too large for a layer.

use super::*;

fn texture(width: u32, height: u32, rgba: &[u8]) -> Texture {
    let pixels = (width * height) as usize;
    Texture {
        width,
        height,
        rgba: rgba.iter().copied().cycle().take(pixels * 4).collect(),
    }
}

fn pixel(layer: &MaterialLayer, index: usize) -> [u8; 4] {
    let LayerContent::Bitmap(bitmap) = &layer.content else {
        panic!("the material has a texture");
    };
    bitmap.rgba8_pixel(index)
}

#[test]
fn a_white_factor_leaves_the_texture_as_it_is() {
    let layer = bake(
        [1.0; 4],
        Alpha::Blend,
        Some(texture(2, 2, &[10, 120, 250, 77])),
    )
    .expect("a layer");
    assert_eq!(layer.size, [2, 2]);
    assert_eq!(pixel(&layer, 3), [10, 120, 250, 77]);
}

#[test]
fn the_factor_multiplies_in_linear_light_and_the_alpha_multiplies_as_it_is() {
    // A half of the light of white is 188 in sRGB, and a half of the alpha is 128
    let layer = bake(
        [0.5, 1.0, 0.0, 0.5],
        Alpha::Blend,
        Some(texture(1, 1, &[255, 255, 255, 255])),
    )
    .expect("a layer");
    assert_eq!(pixel(&layer, 0), [188, 255, 0, 128]);
}

#[test]
fn an_opaque_material_ignores_the_alpha_of_its_texture() {
    let layer = bake([1.0; 4], Alpha::Opaque, Some(texture(1, 1, &[9, 9, 9, 3]))).expect("a layer");
    assert_eq!(pixel(&layer, 0)[3], 255);
}

#[test]
fn a_masked_material_shows_where_the_alpha_reaches_the_cutoff_and_nowhere_else() {
    let mut mask = texture(2, 1, &[0, 0, 0, 0]);
    mask.rgba[3] = 127;
    mask.rgba[7] = 128;
    let layer = bake([1.0; 4], Alpha::Mask(0.5), Some(mask)).expect("a layer");
    // 127 / 255 is below a half and 128 / 255 is above it
    assert_eq!([pixel(&layer, 0)[3], pixel(&layer, 1)[3]], [0, 255]);
}

#[test]
fn a_material_with_no_texture_is_a_flat_color_in_a_small_layer() {
    let layer = bake([0.0, 1.0, 0.2140, 0.25], Alpha::Blend, None).expect("a layer");
    assert_eq!(layer.size, [FLAT_SIDE; 2]);
    let LayerContent::Color([red, green, blue, alpha]) = layer.content else {
        panic!("a flat color");
    };
    assert_eq!((red, green, alpha), (0.0, 1.0, 0.25));
    // 0.214 in linear light is 0.5 in sRGB
    assert!((blue - 0.5).abs() < 1e-3, "{blue}");
    let masked = bake([1.0, 1.0, 1.0, 0.4], Alpha::Mask(0.5), None).expect("a layer");
    assert!(matches!(
        masked.content,
        LayerContent::Color([_, _, _, 0.0])
    ));
}

#[test]
fn a_texture_wider_than_a_layer_may_be_is_halved_until_it_fits() {
    // 4096 by 20 halves once to 2048 by 10
    let layer = bake(
        [1.0; 4],
        Alpha::Blend,
        Some(texture(4096, 20, &[200, 100, 50, 255])),
    )
    .expect("a layer");
    assert_eq!(layer.size, [2048, 10]);
    assert_eq!(pixel(&layer, 0), [200, 100, 50, 255]);
}

#[test]
fn halving_averages_each_block_of_four_pixels() {
    let mut four = texture(2, 2, &[0, 0, 0, 255]);
    four.rgba[0..4].copy_from_slice(&[255, 0, 0, 255]);
    four.rgba[4..8].copy_from_slice(&[0, 255, 0, 255]);
    let half = halved(&four);
    assert_eq!((half.width, half.height), (1, 1));
    // 255 over four pixels is 63.75, which rounds to 64
    assert_eq!(half.rgba, [64, 64, 0, 255]);
}
