#![allow(dead_code)]

use cranpose::{
    composable,
    liquid::prelude::*,
    widgets::{Box as CBox, BoxSpec},
    Color, Modifier, Size,
};

/// The striped ground a glass runner puts its control on.
///
/// Vertical bars of alternating luminance: glass that refracts or blurs its
/// backdrop moves the edges between them, so a screenshot shows what a flat
/// fill would hide.
pub const STRIPE_WIDTH: f32 = 8.0;
pub const DARK_STRIPE: Color = Color(0.05, 0.05, 0.08, 1.0);
pub const LIGHT_STRIPE: Color = Color(0.92, 0.94, 0.99, 1.0);

/// A themed window-sized page holding `content` over striped ground.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidStripedStage(width: u32, height: u32, content: impl FnMut() + 'static) {
    LiquidStage(width, height, move || {
        Stripes(width, height);
        content();
    });
}

/// A themed window-sized page holding `content` over nothing.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidStage(width: u32, height: u32, content: impl FnMut() + 'static) {
    LiquidTheme(LiquidThemeSpec::default(), move || {
        CBox(
            Modifier::empty().size(Size {
                width: width as f32,
                height: height as f32,
            }),
            BoxSpec::default(),
            content,
        );
    });
}

#[composable]
#[allow(non_snake_case)]
fn Stripes(width: u32, height: u32) {
    let count = (width as f32 / STRIPE_WIDTH).ceil() as usize;
    for index in 0..count {
        let color = if index % 2 == 0 {
            DARK_STRIPE
        } else {
            LIGHT_STRIPE
        };
        CBox(
            Modifier::empty()
                .absolute_offset(index as f32 * STRIPE_WIDTH, 0.0)
                .size(Size {
                    width: STRIPE_WIDTH,
                    height: height as f32,
                })
                .background(color),
            BoxSpec::default(),
            || {},
        );
    }
}
