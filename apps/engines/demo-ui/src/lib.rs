mod hud;
mod window;

use cranpose::{
    liquid::{LiquidThemeSpec, SchemeMode},
    prelude::*,
    text::TextUnit,
};
pub use hud::{Hud, HudBridge, HudCommand, HudStates, Stats};
pub use window::{
    EngineInfo, EngineWindow, HostedEngineWindow, TargetSize, WindowBridge, WindowStates,
    rememberWindowStates,
};

#[derive(Clone, Copy, PartialEq)]
pub struct Controls {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub sun: f32,
    pub bloom: f32,
    pub spin: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.42,
            distance: 13.0,
            sun: 0.35,
            bloom: 0.3,
            spin: true,
        }
    }
}

const MUTED: Color = Color::from_rgb_u8(200, 206, 220);

fn text_style(size: f32, color: Color) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(color),
        font_size: TextUnit::Sp(size),
        ..Default::default()
    })
}

fn dark_theme() -> LiquidThemeSpec {
    LiquidThemeSpec {
        scheme: SchemeMode::Dark,
        ..LiquidThemeSpec::default()
    }
}
