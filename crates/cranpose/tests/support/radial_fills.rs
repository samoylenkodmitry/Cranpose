// A screen of rounded fills whose brush differs along each row, which the
// pixel renderer paints pixel by pixel: radial gradients, and every third
// row a horizontal gradient blended with `Multiply`. The gradients move
// every frame, so each frame paints every fill again. No other screen
// reaches that path of a fill. The framebuffer bench shows the screen with
// `SCREEN=radial`.

use cranpose::prelude::*;

use super::frame_time::frame_time;

const ROWS: u64 = 10;
const ROW_HEIGHT: f32 = 38.0;
const RADIUS: f32 = 12.0;
// Each row's gradient crosses the row in two seconds, a step ahead of the
// row above.
const PERIOD_MS: u64 = 2_000;
const STEP_MS: u64 = 173;

const BACKGROUND: Color = Color::from_rgb_u8(15, 18, 32);
const INNER: Color = Color::from_rgb_u8(247, 37, 133);
const OUTER: Color = Color::from_rgb_u8(67, 97, 238);

#[composable]
pub fn RadialFills() {
    let time = frame_time(|_| {});
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(BACKGROUND)
            .padding(8.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            for index in 0..ROWS {
                Box(
                    Modifier::empty()
                        .fill_max_width()
                        .height(ROW_HEIGHT)
                        .draw_behind(move |scope| {
                            let size = scope.size();
                            let at = (time.get() + STEP_MS * index) % PERIOD_MS;
                            let shift = at as f32 / PERIOD_MS as f32;
                            let radii = CornerRadii::uniform(RADIUS);
                            let colors = vec![INNER, OUTER];
                            if index % 3 == 2 {
                                scope.draw_round_rect_blend(
                                    Brush::horizontal_gradient(
                                        colors,
                                        size.width * shift,
                                        size.width,
                                    ),
                                    radii,
                                    BlendMode::Multiply,
                                );
                            } else {
                                let center = Point::new(size.width * shift, size.height / 2.0);
                                scope.draw_round_rect(
                                    Brush::radial_gradient(colors, center, size.width / 2.0),
                                    radii,
                                );
                            }
                        }),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}
