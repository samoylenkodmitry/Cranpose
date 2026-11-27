use super::*;
use crate::handle::{HANDLE_GAP, HANDLE_RADIUS};

#[test]
fn the_projector_s_middle_is_where_the_handle_stands() {
    assert!((HANDLE_DROP - (HANDLE_GAP + HANDLE_RADIUS) * 1000.0).abs() < 1e-3);
}

#[test]
fn the_box_of_a_page_holds_its_handle_and_its_stage_close_to_it() {
    let size = [620.0, 1100.0];
    let bare = hologram_extent(size, (HologramStage::NONE, false), PageObjects::Any);
    assert_eq!(bare, [350.0, 590.0, 40.0]);
    let carried = hologram_extent(size, (HologramStage::NONE, true), PageObjects::Any);
    assert!(
        carried[1] >= 550.0 + HANDLE_DROP + CORE / 2.0,
        "{carried:?}"
    );
    let page = hologram_extent(size, (HologramStage::PAGE, true), PageObjects::Any);
    // The stage stands within a sixth of the page's width beside it and close to its plane
    assert!(page[0] <= 310.0 + 620.0 / 6.0 + 40.0, "{page:?}");
    assert!(page[2] <= 200.0, "{page:?}");
}

#[test]
fn a_still_page_of_gems_takes_a_box_little_larger_than_itself() {
    // A gem of 30 dp on a page of its size, as on a hand
    let gem = hologram_extent(
        [30.0, 30.0],
        (HologramStage::NONE, false),
        PageObjects::StillGems,
    );
    assert_eq!(gem, [25.0, 25.0, 25.0]);
    let any = hologram_extent([30.0, 30.0], (HologramStage::NONE, false), PageObjects::Any);
    let volume = |extent: [f32; 3]| extent.iter().product::<f32>();
    assert!(volume(gem) * 5.0 < volume(any), "{gem:?} against {any:?}");
    // A handle still stands under a movable page
    let carried = hologram_extent(
        [30.0, 30.0],
        (HologramStage::NONE, true),
        PageObjects::StillGems,
    );
    assert!(carried[1] >= 15.0 + HANDLE_DROP + CORE / 2.0, "{carried:?}");
}
