use std::rc::Rc;

use cranpose::prelude::*;

use super::{rememberFrameSeconds, text_style};
use crate::data::{self, Quote, fixed2};

const UP: Color = Color::from_rgb_u8(0x16, 0xA3, 0x4A);
const DOWN: Color = Color::from_rgb_u8(0xDC, 0x26, 0x26);
const INK: Color = Color::from_rgb_u8(0x11, 0x18, 0x27);

/// `rows` quotes share the screen height; past 24 rows the type shrinks with
/// them so each row still fits.
#[composable]
pub fn TickerScreen(rows: usize) {
    let quotes: Rc<[Rc<Quote>]> = remember(|| {
        data::quotes(rows)
            .into_iter()
            .map(Rc::new)
            .collect::<Rc<[_]>>()
    })
    .with(|quotes| quotes.clone());
    let seconds = rememberFrameSeconds();
    let scale = (24.0 / rows as f32).min(1.0);
    Column(
        Modifier::empty()
            .fill_max_width()
            .weight(1.0)
            .padding_vertical(8.0),
        ColumnSpec::default(),
        move || {
            for quote in quotes.iter() {
                QuoteRow(quote.clone(), seconds, scale);
            }
        },
    );
}

#[composable]
fn QuoteRow(quote: Rc<Quote>, seconds: MutableState<f32>, scale: f32) {
    // The only read of the frame clock: each row recomposes on its own.
    let wave = (seconds.get() * quote.speed + quote.phase).sin();
    let price = quote.base * (1.0 + 0.04 * wave);
    let change = 4.0 * wave;
    let color = if change >= 0.0 { UP } else { DOWN };
    Row(
        Modifier::empty()
            .fill_max_width()
            .weight(1.0)
            .padding_horizontal(16.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                quote.symbol.clone(),
                Modifier::empty().width(64.0),
                text_style(14.0 * scale, INK, true),
            );
            Text(
                fixed2(price, false),
                Modifier::empty().width(90.0),
                text_style(14.0 * scale, INK, false),
            );
            Text(
                fixed2(change, true) + "%",
                Modifier::empty().width(72.0),
                text_style(13.0 * scale, color, false),
            );
            Box(
                Modifier::empty()
                    .size_points(8.0 + 72.0 * (0.5 + 0.5 * wave), 10.0 * scale)
                    .background(color)
                    .rounded_corners(5.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}
