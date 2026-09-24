use super::{ATLAS_SIZE_STEP, padded_dimension};

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
