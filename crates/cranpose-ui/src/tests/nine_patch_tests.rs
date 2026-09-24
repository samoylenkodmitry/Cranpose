use super::*;

#[test]
fn only_a_tiled_fill_repeats() {
    assert!(PatchFill::Tile.is_tiled());
    assert!(!PatchFill::Stretch.is_tiled());
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn insets_cannot_be_negative_or_unmeasurable() {
    let insets = NinePatchInsets::new(-4.0, f32::NAN, 6.0, f32::INFINITY);
    assert_eq!(insets.left, 0.0);
    assert_eq!(insets.top, 0.0);
    assert_eq!(insets.right, 6.0);
    assert_eq!(insets.bottom, 0.0);
    assert_eq!(NinePatchInsets::uniform(3.0).left, 3.0);
}

#[test]
fn insets_scale_with_the_source_they_were_measured_on() {
    let insets = NinePatchInsets::uniform(4.0).scaled(2.0);
    assert_eq!(insets, NinePatchInsets::uniform(8.0));
    assert_eq!(
        NinePatchInsets::uniform(4.0).scaled(0.0),
        NinePatchInsets::uniform(4.0)
    );
}

#[test]
fn insets_that_leave_no_middle_do_not_fit() {
    let source = Size::new(20.0, 20.0);
    assert!(NinePatchInsets::uniform(4.0).fit(source));
    assert!(!NinePatchInsets::uniform(10.0).fit(source));
    assert!(!NinePatchInsets::uniform(12.0).fit(source));
}

#[test]
fn a_whole_number_of_tiles_covers_the_destination_exactly() {
    let quads = tile_quads(rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 0.0, 20.0, 20.0));
    assert_eq!(quads.len(), 4);
    assert_eq!(
        tile_count(rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 0.0, 20.0, 20.0)),
        4
    );
    assert_eq!(quads[0].destination, rect(0.0, 0.0, 10.0, 10.0));
    assert_eq!(quads[3].destination, rect(10.0, 10.0, 10.0, 10.0));
    assert!(quads.iter().all(|quad| quad.source.width == 10.0));
}

#[test]
fn a_partial_tile_is_clipped_rather_than_squeezed() {
    let quads = tile_quads(rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 0.0, 25.0, 10.0));
    assert_eq!(quads.len(), 3);
    let last = quads[2];
    assert_eq!(last.destination, rect(20.0, 0.0, 5.0, 10.0));
    assert_eq!(
        last.source,
        rect(0.0, 0.0, 5.0, 10.0),
        "the clipped tile shows the leading part of the source at 1:1"
    );
}

#[test]
fn tiling_reads_from_the_region_it_was_given_not_the_whole_atlas() {
    let quads = tile_quads(rect(64.0, 32.0, 8.0, 8.0), rect(0.0, 0.0, 16.0, 8.0));
    assert_eq!(quads.len(), 2);
    assert!(
        quads
            .iter()
            .all(|quad| quad.source.x == 64.0 && quad.source.y == 32.0)
    );
}

#[test]
fn nothing_is_drawn_for_a_source_or_destination_with_no_area() {
    assert!(tile_quads(rect(0.0, 0.0, 0.0, 10.0), rect(0.0, 0.0, 20.0, 20.0)).is_empty());
    assert!(tile_quads(rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 0.0, 20.0, 0.0)).is_empty());
    assert_eq!(
        tile_count(rect(0.0, 0.0, 0.0, 0.0), rect(0.0, 0.0, 8.0, 8.0)),
        0
    );
    assert!(
        nine_patch_quads(
            rect(0.0, 0.0, 0.0, 0.0),
            rect(0.0, 0.0, 20.0, 20.0),
            NinePatchInsets::uniform(4.0),
            PatchFill::Stretch,
            PatchFill::Stretch,
        )
        .is_empty()
    );
}

#[test]
fn a_stretched_nine_patch_keeps_its_corners_and_grows_the_rest() {
    let quads = nine_patch_quads(
        rect(0.0, 0.0, 30.0, 30.0),
        rect(0.0, 0.0, 100.0, 60.0),
        NinePatchInsets::uniform(10.0),
        PatchFill::Stretch,
        PatchFill::Stretch,
    );
    assert_eq!(quads.len(), 9);

    let top_left = quads[0];
    assert_eq!(top_left.source, rect(0.0, 0.0, 10.0, 10.0));
    assert_eq!(
        top_left.destination,
        rect(0.0, 0.0, 10.0, 10.0),
        "a corner is drawn at its own size"
    );

    let bottom_right = quads[8];
    assert_eq!(bottom_right.source, rect(20.0, 20.0, 10.0, 10.0));
    assert_eq!(bottom_right.destination, rect(90.0, 50.0, 10.0, 10.0));

    let middle = quads[4];
    assert_eq!(middle.source, rect(10.0, 10.0, 10.0, 10.0));
    assert_eq!(middle.destination, rect(10.0, 10.0, 80.0, 40.0));
}

#[test]
fn the_patches_cover_the_destination_without_gaps_or_overlap() {
    let destination = rect(5.0, 7.0, 100.0, 60.0);
    let quads = nine_patch_quads(
        rect(0.0, 0.0, 30.0, 30.0),
        destination,
        NinePatchInsets::new(10.0, 8.0, 6.0, 4.0),
        PatchFill::Stretch,
        PatchFill::Stretch,
    );
    let area: f32 = quads
        .iter()
        .map(|quad| quad.destination.width * quad.destination.height)
        .sum();
    assert!(
        (area - destination.width * destination.height).abs() < 0.001,
        "nine patches must tile the destination exactly, covered {area}"
    );
}

#[test]
fn a_tiled_nine_patch_repeats_its_edges_and_middle() {
    let quads = nine_patch_quads(
        rect(0.0, 0.0, 30.0, 30.0),
        rect(0.0, 0.0, 50.0, 30.0),
        NinePatchInsets::uniform(10.0),
        PatchFill::Tile,
        PatchFill::Tile,
    );
    let corners = quads
        .iter()
        .filter(|quad| quad.destination.width == 10.0 && quad.destination.height == 10.0)
        .count();
    assert!(corners >= 4);
    assert!(
        quads
            .iter()
            .all(|quad| quad.source.width <= 10.0 && quad.source.height <= 10.0),
        "a tiled patch never reads more than one source tile at a time"
    );
    assert!(
        quads.len() > 9,
        "tiling produces more draws than the nine stretched patches"
    );
}

#[test]
fn a_destination_too_small_for_the_corners_falls_back_to_a_plain_scale() {
    let quads = nine_patch_quads(
        rect(0.0, 0.0, 30.0, 30.0),
        rect(0.0, 0.0, 12.0, 12.0),
        NinePatchInsets::uniform(10.0),
        PatchFill::Stretch,
        PatchFill::Stretch,
    );
    assert_eq!(quads.len(), 1);
    assert_eq!(quads[0].destination, rect(0.0, 0.0, 12.0, 12.0));
    assert_eq!(quads[0].source, rect(0.0, 0.0, 30.0, 30.0));
}

#[test]
fn insets_with_no_middle_left_fall_back_to_a_plain_scale() {
    let quads = nine_patch_quads(
        rect(0.0, 0.0, 20.0, 20.0),
        rect(0.0, 0.0, 100.0, 100.0),
        NinePatchInsets::uniform(10.0),
        PatchFill::Stretch,
        PatchFill::Stretch,
    );
    assert_eq!(quads.len(), 1);
}
