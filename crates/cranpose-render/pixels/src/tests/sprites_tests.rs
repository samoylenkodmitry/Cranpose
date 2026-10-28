use std::{rc::Rc, sync::Arc};

use cranpose_core::collections::map::HashSet;
use cranpose_ui::text::{AnnotatedString, TextStyle, font::FontWeight};

use super::*;
use crate::scene::{ShadowDraw, TextDraw};

fn text_draw() -> TextDraw {
    TextDraw {
        rect: Rect {
            x: 0.25,
            y: 0.5,
            width: 80.0,
            height: 20.0,
        },
        snap_anchor: None,
        text: Rc::new(AnnotatedString::from("72 bpm")),
        color: Color(1.0, 1.0, 1.0, 1.0),
        text_style: Arc::new(TextStyle::default()),
        font_size: 14.0,
        scale: 2.0,
        clip: None,
    }
}

fn shadow_draw() -> ShadowDraw {
    ShadowDraw {
        shadow: ShadowRRect {
            bounds: Rect {
                x: 0.5,
                y: 0.25,
                width: 60.0,
                height: 30.0,
            },
            radii: [6.0; 4],
            umbra_inset: 3.0,
            distance_correction: 0.5,
            hole: None,
        },
        snap_anchor: None,
        color: Color(0.0, 0.0, 0.0, 0.3),
        clip: None,
    }
}

fn text_key(draw: TextDraw) -> u64 {
    let mut scene = RasterScene::new();
    scene.texts.push(draw);
    Canonical {
        scene,
        item: RenderItem::Text(0),
    }
    .key()
}

fn shadow_key(draw: ShadowDraw) -> u64 {
    let mut scene = RasterScene::new();
    scene.shadows.push(draw);
    Canonical {
        scene,
        item: RenderItem::Shadow(0),
    }
    .key()
}

// Asserts every key differs from every other.
fn assert_distinct(keys: &[u64]) {
    let distinct: HashSet<u64> = keys.iter().copied().collect();
    assert_eq!(distinct.len(), keys.len(), "keys: {keys:?}");
}

#[test]
fn a_text_sprite_key_changes_with_each_setting_of_its_draw() {
    let bold = TextStyle {
        span_style: cranpose_ui::text::style::SpanStyle {
            font_weight: Some(FontWeight::BOLD),
            ..TextStyle::default().span_style
        },
        ..TextStyle::default()
    };
    assert_eq!(text_key(text_draw()), text_key(text_draw()));
    assert_distinct(&[
        text_key(text_draw()),
        text_key(TextDraw {
            rect: Rect {
                width: 81.0,
                ..text_draw().rect
            },
            ..text_draw()
        }),
        text_key(TextDraw {
            snap_anchor: Some(Point::new(0.25, 0.5)),
            ..text_draw()
        }),
        text_key(TextDraw {
            text: Rc::new(AnnotatedString::from("73 bpm")),
            ..text_draw()
        }),
        text_key(TextDraw {
            color: Color(1.0, 1.0, 1.0, 0.5),
            ..text_draw()
        }),
        text_key(TextDraw {
            text_style: Arc::new(bold),
            ..text_draw()
        }),
        text_key(TextDraw {
            font_size: 15.0,
            ..text_draw()
        }),
        text_key(TextDraw {
            scale: 3.0,
            ..text_draw()
        }),
        text_key(TextDraw {
            clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            }),
            ..text_draw()
        }),
    ]);
}

#[test]
fn a_shadow_sprite_key_changes_with_each_setting_of_its_draw() {
    let with_shadow = |change: fn(&mut ShadowRRect)| {
        let mut draw = shadow_draw();
        change(&mut draw.shadow);
        shadow_key(draw)
    };
    assert_eq!(shadow_key(shadow_draw()), shadow_key(shadow_draw()));
    assert_distinct(&[
        shadow_key(shadow_draw()),
        with_shadow(|shadow| shadow.bounds.height = 31.0),
        with_shadow(|shadow| shadow.radii[2] = 5.0),
        with_shadow(|shadow| shadow.umbra_inset = 2.0),
        with_shadow(|shadow| shadow.distance_correction = 0.25),
        with_shadow(|shadow| {
            shadow.hole = Some(ShadowHole {
                inset: 1.0,
                radius: 4.0,
            });
        }),
        with_shadow(|shadow| {
            shadow.hole = Some(ShadowHole {
                inset: 1.0,
                radius: 5.0,
            });
        }),
        shadow_key(ShadowDraw {
            snap_anchor: Some(Point::new(0.5, 0.25)),
            ..shadow_draw()
        }),
        shadow_key(ShadowDraw {
            color: Color(0.0, 0.0, 0.0, 0.4),
            ..shadow_draw()
        }),
        shadow_key(ShadowDraw {
            clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 30.0,
            }),
            ..shadow_draw()
        }),
    ]);
}
