use super::*;

fn area(bands: &[DeviceRect4]) -> f32 {
    bands.iter().map(|band| band.2 * band.3).sum()
}

fn disjoint(bands: &[DeviceRect4]) -> bool {
    bands.iter().enumerate().all(|(index, a)| {
        bands
            .iter()
            .skip(index + 1)
            .all(|b| intersect_device_rects(*a, *b).is_none())
    })
}

#[test]
fn an_interior_occluder_leaves_four_disjoint_bands_that_tile_the_ring() {
    let bands = shadow_bands((0.0, 0.0, 100.0, 80.0), Some((20.0, 10.0, 50.0, 40.0)));
    assert_eq!(bands.len(), 4);
    assert!(disjoint(&bands));
    assert_eq!(area(&bands), 100.0 * 80.0 - 50.0 * 40.0);
}

#[test]
fn an_occluder_outside_the_coverage_changes_nothing() {
    let coverage = (0.0, 0.0, 100.0, 80.0);
    let bands = shadow_bands(coverage, Some((200.0, 200.0, 10.0, 10.0)));
    assert_eq!(bands.as_slice(), &[coverage]);
}

#[test]
fn an_occluder_swallowing_the_coverage_leaves_nothing_to_draw() {
    let bands = shadow_bands((10.0, 10.0, 20.0, 20.0), Some((0.0, 0.0, 100.0, 100.0)));
    assert!(bands.is_empty());
}

#[test]
fn a_fractional_occluder_shrinks_inward_so_no_covered_pixel_is_skipped() {
    let bands = shadow_bands((0.0, 0.0, 100.0, 80.0), Some((20.4, 10.6, 50.2, 40.1)));
    assert!(disjoint(&bands));
    let ring = 100.0 * 80.0 - (70.0 - 21.0) * (50.0 - 11.0);
    assert_eq!(area(&bands), ring);
}
