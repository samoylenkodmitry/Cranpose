use std::sync::atomic::{AtomicBool, Ordering};

use cranpose_liquid::{Glass, GlassButton, GlassButtonSpec, LiquidTheme, LiquidThemeSpec};
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 200;
const BUTTON: [f32; 4] = [80.0, 60.0, 160.0, 52.0];

static SHADOW: AtomicBool = AtomicBool::new(true);

/// A glass button whose glass is not clipped to its shape: the glass shader
/// draws the button's drop shadow itself, around the button and past its box.
#[composable]
fn GlassButtonPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        FramePage(
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Color(0.96, 0.93, 0.9, 1.0),
            || {
                let glass = Glass::regular()
                    .no_clip()
                    .shadow(SHADOW.load(Ordering::Relaxed));
                GlassButton(
                    rect_modifier(BUTTON),
                    GlassButtonSpec::glass().with_glass(glass),
                    || {},
                    || {},
                );
            },
        );
    });
}

fn capture(shadow: bool) -> Option<Vec<u8>> {
    SHADOW.store(shadow, Ordering::Relaxed);
    support::warm_app_frame(GlassButtonPage, FRAME_WIDTH, FRAME_HEIGHT)
        .map(|(frame, _)| frame.pixels)
}

#[test]
fn a_glass_buttons_own_shadow_reaches_past_its_box() {
    let Some(with_shadow) = capture(true) else {
        return;
    };
    let without_shadow = capture(false).expect("headless WGPU init failed mid-suite");
    let (outside_differing, outside_max_delta) =
        support::difference_outside(FRAME_WIDTH, &with_shadow, &without_shadow, BUTTON);
    eprintln!(
        "shadow on vs off outside the button: {outside_differing} pixels differ, by at most \
         {outside_max_delta}"
    );
    assert!(
        outside_differing > 500 && outside_max_delta >= 4,
        "the glass's own drop shadow must darken the page around the button; \
         {outside_differing} pixels outside it differ by at most {outside_max_delta} levels, so \
         the composite cut the shadow at the button's rectangle"
    );
}
