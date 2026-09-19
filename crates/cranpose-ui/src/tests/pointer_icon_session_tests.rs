use cranpose_ui_graphics::{CursorIcon, ImageBitmap};

use super::*;
use crate::render_state::AppContext;

fn custom_icon() -> PointerIcon {
    PointerIcon::custom(
        ImageBitmap::from_rgba8(4, 4, vec![255; 64]).expect("bitmap"),
        1,
        2,
    )
    .expect("icon")
}

#[test]
fn a_fresh_session_holds_the_default_icon_and_no_change() {
    let context = AppContext::new();
    context.enter(|| {
        assert_eq!(current_pointer_icon(), PointerIcon::DEFAULT);
        assert_eq!(take_pointer_icon_change(), None);
    });
}

#[test]
fn setting_a_new_icon_yields_one_change() {
    let context = AppContext::new();
    context.enter(|| {
        set_pointer_icon(PointerIcon::POINTER);
        assert_eq!(current_pointer_icon(), PointerIcon::POINTER);
        assert_eq!(take_pointer_icon_change(), Some(PointerIcon::POINTER));
        assert_eq!(take_pointer_icon_change(), None);
    });
}

#[test]
fn re_setting_the_same_icon_reports_no_change() {
    let context = AppContext::new();
    context.enter(|| {
        set_pointer_icon(PointerIcon::TEXT);
        assert_eq!(take_pointer_icon_change(), Some(PointerIcon::TEXT));
        set_pointer_icon(PointerIcon::TEXT);
        assert_eq!(take_pointer_icon_change(), None);
    });
}

#[test]
fn the_latest_icon_wins_when_the_platform_has_not_polled() {
    let context = AppContext::new();
    context.enter(|| {
        set_pointer_icon(PointerIcon::POINTER);
        set_pointer_icon(PointerIcon::System(CursorIcon::Crosshair));
        assert_eq!(
            take_pointer_icon_change(),
            Some(PointerIcon::System(CursorIcon::Crosshair))
        );
        assert_eq!(take_pointer_icon_change(), None);
    });
}

#[test]
fn custom_icons_round_trip_through_the_session() {
    let context = AppContext::new();
    context.enter(|| {
        let icon = custom_icon();
        set_pointer_icon(icon.clone());
        assert_eq!(take_pointer_icon_change(), Some(icon.clone()));
        set_pointer_icon(icon);
        assert_eq!(take_pointer_icon_change(), None);
    });
}

#[test]
fn a_refresh_offers_the_icon_the_region_already_asked_for() {
    let context = AppContext::new();
    context.enter(|| {
        set_pointer_icon(PointerIcon::POINTER);
        assert_eq!(take_pointer_icon_change(), Some(PointerIcon::POINTER));
        assert_eq!(take_pointer_icon_change(), None);

        refresh_pointer_icon();
        assert_eq!(
            take_pointer_icon_change(),
            Some(PointerIcon::POINTER),
            "coming back to the window re-applies the region's own cursor"
        );
    });
}

#[test]
fn a_refresh_on_a_window_that_asked_for_nothing_restores_the_default() {
    let context = AppContext::new();
    context.enter(|| {
        refresh_pointer_icon();
        assert_eq!(take_pointer_icon_change(), Some(PointerIcon::DEFAULT));
    });
}

#[test]
fn each_app_context_carries_its_own_icon() {
    let first = AppContext::new();
    let second = AppContext::new();
    first.enter(|| set_pointer_icon(PointerIcon::POINTER));
    second.enter(|| {
        assert_eq!(current_pointer_icon(), PointerIcon::DEFAULT);
        assert_eq!(take_pointer_icon_change(), None);
    });
    first.enter(|| {
        assert_eq!(take_pointer_icon_change(), Some(PointerIcon::POINTER));
    });
}
