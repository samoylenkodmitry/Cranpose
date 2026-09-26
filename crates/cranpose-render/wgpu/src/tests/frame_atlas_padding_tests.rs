use super::{ATLAS_SIZE_STEP, AtlasPacker, padded_dimension};

#[test]
fn padded_dimensions_step_by_an_eighth_of_their_magnitude_and_never_exceed_the_limit() {
    assert_eq!(padded_dimension(1, 4096), ATLAS_SIZE_STEP);
    assert_eq!(padded_dimension(17, 4096), 32);
    assert_eq!(padded_dimension(300, 4096), 320);
    assert_eq!(padded_dimension(1080, 4096), 1280);
    assert_eq!(padded_dimension(2072, 4096), 2560);
    assert_eq!(padded_dimension(4000, 4096), 4096);
    assert_eq!(padded_dimension(2100, 3000), 2560);
    assert_eq!(padded_dimension(2900, 3000), 3000);
    assert_eq!(padded_dimension(24, 20), 20);
}

#[test]
fn a_packer_with_a_shelf_width_opens_shelves_below_instead_of_widening() {
    let mut packer = AtlasPacker::new(4096).with_shelf_width(100);
    let placed: Vec<(u32, u32)> = (0..4)
        .filter_map(|_| packer.place(40, 30))
        .map(|placement| (placement.x, placement.y))
        .collect();
    assert_eq!(placed, vec![(0, 0), (40, 0), (0, 30), (40, 30)]);
    let wide = packer
        .place(300, 10)
        .expect("wider than a shelf still fits");
    assert_eq!((wide.x, wide.y), (0, 60), "it opens a shelf of its own");
    assert_eq!(packer.atlases.len(), 1);
    assert_eq!(
        (packer.atlases[0].width, packer.atlases[0].height),
        (300, 70)
    );
    assert_eq!(
        AtlasPacker::new(64).with_shelf_width(1000).shelf_width,
        64,
        "a shelf never runs past the limit"
    );
}
