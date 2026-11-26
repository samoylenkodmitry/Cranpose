use super::*;
use crate::handle::{HANDLE_GAP, HANDLE_RADIUS};

#[test]
fn the_projector_s_middle_is_where_the_handle_stands() {
    assert!((HANDLE_DROP - (HANDLE_GAP + HANDLE_RADIUS) * 1000.0).abs() < 1e-3);
}

#[test]
fn the_box_of_a_page_holds_its_handle_and_its_stage_close_to_it() {
    let size = [620.0, 1100.0];
    let bare = hologram_extent(size, (HologramStage::NONE, false));
    assert_eq!(bare, [350.0, 590.0, 40.0]);
    let carried = hologram_extent(size, (HologramStage::NONE, true));
    assert!(
        carried[1] >= 550.0 + HANDLE_DROP + CORE / 2.0,
        "{carried:?}"
    );
    let page = hologram_extent(size, (HologramStage::PAGE, true));
    // The stage stands within a sixth of the page's width beside it and close to its plane
    assert!(page[0] <= 310.0 + 620.0 / 6.0 + 40.0, "{page:?}");
    assert!(page[2] <= 200.0, "{page:?}");
}
