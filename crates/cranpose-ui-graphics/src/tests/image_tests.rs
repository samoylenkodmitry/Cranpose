use super::*;

#[test]
fn the_same_bytes_under_transposed_dimensions_are_different_bitmaps() {
    let pixels = vec![1, 2, 3, 255, 4, 5, 6, 255];
    let wide = ImageBitmap::from_rgba8(2, 1, pixels.clone()).expect("wide bitmap");
    let tall = ImageBitmap::from_rgba8(1, 2, pixels).expect("tall bitmap");

    assert_ne!(wide.id(), tall.id());
}

#[test]
fn one_flipped_bit_deep_in_a_large_bitmap_changes_its_id() {
    let (width, height) = (640u32, 360u32);
    let mut pixels: Vec<u8> = (0..width * height * 4)
        .map(|index| (index.wrapping_mul(2_654_435_761) >> 11) as u8)
        .collect();
    let original = ImageBitmap::from_rgba8_slice(width, height, &pixels).expect("original");
    let last = pixels.len() - 1;
    pixels[last - 4096] ^= 0b0000_0100;
    let flipped = ImageBitmap::from_rgba8_slice(width, height, &pixels).expect("flipped");

    assert_ne!(original.id(), flipped.id());
}

#[test]
fn every_colour_filter_states_itself_as_a_matrix() {
    let identity = ColorFilter::Matrix([
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 0.0,
    ]);
    assert_eq!(
        ColorFilter::matrix(identity.as_matrix()).as_matrix(),
        identity.as_matrix(),
        "a matrix filter is its own matrix"
    );

    let tint = Color(0.25, 0.5, 0.75, 1.0);
    let matrix = ColorFilter::tint(tint).as_matrix();
    for row in 0..4 {
        for column in 0..3 {
            assert_eq!(matrix[row * 5 + column], 0.0, "row {row} column {column}");
        }
        assert_eq!(matrix[row * 5 + 4], 0.0, "row {row} offset");
    }
    assert_eq!(matrix[3], tint.r());
    assert_eq!(matrix[8], tint.g());
    assert_eq!(matrix[13], tint.b());
    assert_eq!(matrix[18], tint.a());

    let modulate = ColorFilter::modulate(Color(0.5, 0.25, 0.125, 1.0)).as_matrix();
    assert_eq!(modulate[0], 0.5);
    assert_eq!(modulate[6], 0.25);
    assert_eq!(modulate[12], 0.125);
    assert_eq!(modulate[18], 1.0);
}

#[test]
fn image_bitmap_ids_do_not_use_process_global_or_allocation_identity() {
    let source = include_str!("../image.rs");
    let image_counter = ["static ", "NEXT_IMAGE_BITMAP_ID"].concat();
    let pixel_pointer = ["Arc::", "as_ptr(&self.pixels)"].concat();

    assert!(
        !source.contains(&image_counter) && !source.contains(&pixel_pointer),
        "image bitmap ids must be derived from bitmap content, not global counters or allocation addresses"
    );
}

#[test]
fn from_rgba8_accepts_valid_data() {
    let bitmap =
        ImageBitmap::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]).expect("valid bitmap");

    assert_eq!(bitmap.width(), 2);
    assert_eq!(bitmap.height(), 1);
    assert_eq!(bitmap.pixels().len(), 8);
    assert!(bitmap.is_opaque());
}

#[test]
fn from_rgba8_tracks_transparency() {
    let bitmap =
        ImageBitmap::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 128]).expect("valid bitmap");

    assert!(!bitmap.is_opaque());
}

#[test]
fn from_rgba8_rejects_zero_dimensions() {
    let err = ImageBitmap::from_rgba8(0, 2, vec![]).expect_err("must fail");
    assert_eq!(err, ImageBitmapError::InvalidDimensions);
}

#[test]
fn from_rgba8_rejects_wrong_pixel_length() {
    let err = ImageBitmap::from_rgba8(2, 2, vec![0; 15]).expect_err("must fail");
    assert_eq!(
        err,
        ImageBitmapError::PixelDataLengthMismatch {
            expected: 16,
            actual: 15,
        }
    );
}

#[test]
fn from_rgba8_slice_accepts_valid_data() {
    let pixels = [255u8, 0, 0, 255];
    let bitmap = ImageBitmap::from_rgba8_slice(1, 1, &pixels).expect("valid bitmap");
    assert_eq!(bitmap.pixels(), &pixels);
}

#[test]
fn ids_are_content_derived() {
    let a = ImageBitmap::from_rgba8(1, 1, vec![0, 0, 0, 255]).expect("bitmap a");
    let a_clone = a.clone();
    let b = ImageBitmap::from_rgba8(1, 1, vec![0, 0, 0, 255]).expect("bitmap b");
    let c = ImageBitmap::from_rgba8(1, 1, vec![0, 0, 1, 255]).expect("bitmap c");
    let d = ImageBitmap::from_rgba8(2, 1, vec![0, 0, 0, 255, 0, 0, 0, 255]).expect("bitmap d");

    assert_eq!(a.id(), a_clone.id());
    assert_eq!(a.id(), b.id());
    assert_ne!(a.id(), c.id());
    assert_ne!(a.id(), d.id());
}

#[test]
fn intrinsic_size_matches_dimensions() {
    let bitmap = ImageBitmap::from_rgba8(3, 4, vec![255; 3 * 4 * 4]).expect("bitmap");
    assert_eq!(bitmap.intrinsic_size(), Size::new(3.0, 4.0));
}

#[test]
fn tint_filter_multiplies_channels() {
    let filter = ColorFilter::modulate(Color::from_rgba_u8(128, 255, 64, 128));
    let tinted = filter.apply_rgba([1.0, 0.5, 1.0, 1.0]);
    assert!((tinted[0] - (128.0 / 255.0)).abs() < 1e-5);
    assert!((tinted[1] - 0.5).abs() < 1e-5);
    assert!((tinted[2] - (64.0 / 255.0)).abs() < 1e-5);
    assert!((tinted[3] - (128.0 / 255.0)).abs() < 1e-5);
}

#[test]
fn tint_constructor_matches_variant() {
    let color = Color::from_rgba_u8(10, 20, 30, 40);
    assert_eq!(ColorFilter::tint(color), ColorFilter::Tint(color));
}

#[test]
fn tint_filter_uses_src_in_behavior() {
    let filter = ColorFilter::tint(Color::from_rgba_u8(255, 128, 0, 128));
    let tinted = filter.apply_rgba([0.2, 0.4, 0.8, 0.25]);
    assert!((tinted[0] - 0.25).abs() < 1e-5);
    assert!((tinted[1] - (0.25 * 128.0 / 255.0)).abs() < 1e-5);
    assert!(tinted[2].abs() < 1e-5);
    assert!((tinted[3] - (0.25 * 128.0 / 255.0)).abs() < 1e-5);
}

#[test]
fn matrix_filter_transforms_channels() {
    let matrix = [
        1.0, 0.0, 0.0, 0.0, 0.1, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 0.0,
    ];
    let filter = ColorFilter::matrix(matrix);
    let transformed = filter.apply_rgba([0.2, 0.6, 0.9, 0.4]);
    assert!((transformed[0] - 0.3).abs() < 1e-5);
    assert!((transformed[1] - 0.3).abs() < 1e-5);
    assert!((transformed[2] - 0.4).abs() < 1e-5);
    assert!((transformed[3] - 0.4).abs() < 1e-5);
}

#[test]
fn filter_compose_applies_in_order() {
    let first = ColorFilter::modulate(Color::from_rgba_u8(128, 255, 255, 255));
    let second = ColorFilter::tint(Color::from_rgba_u8(255, 0, 0, 255));
    let chained = first.compose(second);
    let direct_second = second.apply_rgba(first.apply_rgba([0.8, 0.4, 0.2, 0.5]));
    let composed = chained.apply_rgba([0.8, 0.4, 0.2, 0.5]);
    assert!((direct_second[0] - composed[0]).abs() < 1e-5);
    assert!((direct_second[1] - composed[1]).abs() < 1e-5);
    assert!((direct_second[2] - composed[2]).abs() < 1e-5);
    assert!((direct_second[3] - composed[3]).abs() < 1e-5);
}
