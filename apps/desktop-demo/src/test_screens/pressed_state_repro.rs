//! Repro screen for pressed-state visuals driven by `pointer_input` Down events.
//!
//! Mirrors a Winamp-style sprite button: a Canvas draws one of two sprite-sheet
//! regions depending on a `useState(bool)` that flips on pointer Down/Up.

use cranpose_ui::{
    composable, Box, BoxSpec, Brush, Canvas, Color, CornerRadii, ImageBitmap, Modifier,
    PointerEventKind, PointerInputScope, Rect, Text, TextStyle,
};

pub const SPRITE_WIDTH: f32 = 120.0;
pub const SPRITE_HEIGHT: f32 = 60.0;
pub const SPRITE_OFFSET_X: f32 = 40.0;
pub const SPRITE_OFFSET_Y: f32 = 100.0;
/// Left sprite cell (normal): solid blue.
pub const NORMAL_RGBA: [u8; 4] = [20, 40, 220, 255];
/// Right sprite cell (pressed): solid red.
pub const PRESSED_RGBA: [u8; 4] = [220, 40, 20, 255];

fn sprite_sheet() -> ImageBitmap {
    let width = SPRITE_WIDTH as u32 * 2;
    let height = SPRITE_HEIGHT as u32;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            let rgba = if x < SPRITE_WIDTH as u32 {
                NORMAL_RGBA
            } else {
                PRESSED_RGBA
            };
            pixels.extend_from_slice(&rgba);
        }
    }
    ImageBitmap::from_rgba8(width, height, pixels).expect("valid sprite sheet bitmap")
}

#[allow(non_snake_case)]
#[composable]
pub fn PressedStateReproScreen() {
    let is_pressed = cranpose_core::useState(|| false);
    let sheet: ImageBitmap = cranpose_core::remember(sprite_sheet).with(|b| b.clone());
    let src = if is_pressed.get() {
        Rect {
            x: SPRITE_WIDTH,
            y: 0.0,
            width: SPRITE_WIDTH,
            height: SPRITE_HEIGHT,
        }
    } else {
        Rect {
            x: 0.0,
            y: 0.0,
            width: SPRITE_WIDTH,
            height: SPRITE_HEIGHT,
        }
    };

    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.05, 0.05, 0.08, 1.0)),
        BoxSpec::default(),
        move || {
            Text(
                "Pressed State Repro",
                Modifier::empty().padding(8.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.2, 0.2, 0.3, 1.0)),
                        CornerRadii::uniform(8.0),
                    );
                }),
                TextStyle::default(),
            );

            let sheet = sheet.clone();
            Canvas(
                Modifier::empty()
                    .size_points(SPRITE_WIDTH, SPRITE_HEIGHT)
                    .absolute_offset(SPRITE_OFFSET_X, SPRITE_OFFSET_Y)
                    .pointer_input((), move |scope: PointerInputScope| async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down => {
                                            is_pressed.set(true);
                                            event.consume();
                                        }
                                        PointerEventKind::Up | PointerEventKind::Cancel => {
                                            is_pressed.set(false);
                                            event.consume();
                                        }
                                        _ => {}
                                    }
                                }
                            })
                            .await;
                    }),
                move |scope| {
                    let dst = Rect {
                        x: 0.0,
                        y: 0.0,
                        width: SPRITE_WIDTH,
                        height: SPRITE_HEIGHT,
                    };
                    scope.draw_image_src(sheet.clone(), src, dst, 1.0, None);
                },
            );
        },
    );
}
