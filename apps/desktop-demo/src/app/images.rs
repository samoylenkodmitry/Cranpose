use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, ContentScale,
    CornerRadii, Image, ImageBitmap, LinearArrangement, Modifier, Size, Spacer, Text, TextStyle,
};

const CHESSBOARD_LIGHT: [u8; 4] = [240, 240, 240, 255];
const CHESSBOARD_DARK: [u8; 4] = [36, 54, 72, 255];

pub(crate) fn generate_chessboard_bitmap(tile_size: u32, tiles_per_side: u32) -> ImageBitmap {
    let width = tile_size.max(1) * tiles_per_side.max(1);
    let height = width;
    let mut pixels = vec![0u8; (width * height * 4) as usize];

    for y in 0..height {
        for x in 0..width {
            let tile_x = x / tile_size.max(1);
            let tile_y = y / tile_size.max(1);
            let color = if ((tile_x + tile_y) & 1) == 0 {
                CHESSBOARD_LIGHT
            } else {
                CHESSBOARD_DARK
            };
            let idx = ((y * width + x) * 4) as usize;
            pixels[idx..idx + 4].copy_from_slice(&color);
        }
    }

    ImageBitmap::from_rgba8(width, height, pixels).expect("valid chessboard bitmap")
}

#[composable]
pub(crate) fn images_tab() {
    let chessboard =
        cranpose_core::remember(|| generate_chessboard_bitmap(32, 8)).with(|bitmap| bitmap.clone());

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.10, 0.13, 0.20, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Text(
                "Chessboard Image Demo",
                Modifier::empty()
                    .padding(10.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(14.0),
                TextStyle::default(),
            );

            Text(
                "Generated RGBA bitmap rendered via Image composable.",
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.14, 0.18, 0.30, 0.8))
                    .rounded_corners(12.0),
                TextStyle::default(),
            );

            Spacer(Size::new(0.0, 8.0));

            Box(
                Modifier::empty()
                    .size(Size::new(280.0, 280.0))
                    .rounded_corners(16.0)
                    .draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.0, 0.0, 0.0, 0.25)),
                            CornerRadii::uniform(16.0),
                        );
                    })
                    .padding(12.0),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                {
                    let board = chessboard.clone();
                    move || {
                        Image(
                            board.clone(),
                            Some("Generated chessboard".to_string()),
                            Modifier::empty().fill_max_size(),
                            Alignment::CENTER,
                            ContentScale::Fit,
                            1.0,
                            None,
                        );
                    }
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(bitmap: &ImageBitmap, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * bitmap.width() + x) * 4) as usize;
        let px = bitmap.pixels();
        [px[idx], px[idx + 1], px[idx + 2], px[idx + 3]]
    }

    #[test]
    fn chessboard_bitmap_has_expected_dimensions() {
        let bitmap = generate_chessboard_bitmap(8, 4);
        assert_eq!(bitmap.width(), 32);
        assert_eq!(bitmap.height(), 32);
    }

    #[test]
    fn chessboard_bitmap_alternates_colors() {
        let bitmap = generate_chessboard_bitmap(4, 2);
        assert_eq!(pixel(&bitmap, 1, 1), CHESSBOARD_LIGHT);
        assert_eq!(pixel(&bitmap, 5, 1), CHESSBOARD_DARK);
        assert_eq!(pixel(&bitmap, 1, 5), CHESSBOARD_DARK);
        assert_eq!(pixel(&bitmap, 5, 5), CHESSBOARD_LIGHT);
    }
}
