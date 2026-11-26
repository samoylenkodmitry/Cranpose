use perf_data::*;

use crate::{grid::Grid, skia::*};

fn color([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb(r, g, b)
}

struct TileLayer {
    size: (i32, i32),
    scale: f32,
    grid: Grid,
    image: Image,
}

pub(crate) struct Wall {
    tier: WallTier,

    normal: Typeface,
    bold: Typeface,
    layer: Option<TileLayer>,
}

impl Wall {
    pub fn new(normal: Typeface, bold: Typeface) -> Self {
        let launch = Launch::from_env();
        assert_eq!(launch.scenario, Scenario::Wall);
        Self {
            tier: wall_tier(launch.tier),

            normal,
            bold,
            layer: None,
        }
    }

    fn tiles(&self, size: (i32, i32), scale: f32) -> TileLayer {
        let mut surface = raster_n32_premul(size).expect("wall tile surface");
        let canvas = surface.canvas();
        canvas.clear(color(WALL_BACKGROUND_RGB));
        canvas.scale((scale, scale));
        let grid = Grid::new(self.tier, size.0 as f32 / scale, size.1 as f32 / scale);
        let (w, h) = (self.tier.tile_width, self.tier.tile_height);
        let (ls, vs) = wall_text_size(h);
        let label = Font::new(self.bold.clone(), ls);
        let value = Font::new(self.normal.clone(), vs);
        let pad = wall_padding(h);
        let dot = wall_dot_size(h);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        for index in 0..grid.tiles {
            let (x, y) = grid.origin(index);
            paint.set_color(color(wall_fill_rgb(index)));
            canvas.draw_round_rect(
                Rect::from_xywh(x, y, w, h),
                WALL_CORNER,
                WALL_CORNER,
                &paint,
            );
            let (left, top) = (x + pad, y + pad);
            paint.set_color(color(WALL_INK_RGB));
            canvas.draw_str(wall_label(index), (left, top + ls), &label, &paint);
            canvas.draw_str(
                wall_value(index),
                (left + dot * 1.5, top + ls * 1.17 + vs),
                &value,
                &paint,
            );
            paint.set_color(color(wall_dot_rgb(index)));
            canvas.draw_circle(
                (left + dot / 2.0, top + ls * 1.17 + vs * 0.6),
                dot / 2.0,
                &paint,
            );
        }
        TileLayer {
            size,
            scale,
            grid,
            image: surface.image_snapshot(),
        }
    }

    pub fn draw(&mut self, canvas: &Canvas, width: f32, height: f32, scale: f32, frame: u32) {
        let size = (
            (width * scale).round() as i32,
            (height * scale).round() as i32,
        );
        if size.0 <= 0 || size.1 <= 0 {
            return;
        }
        if self
            .layer
            .as_ref()
            .is_none_or(|layer| layer.size != size || scale != layer.scale)
        {
            self.layer = Some(self.tiles(size, scale));
        }
        let layer = self.layer.as_ref().unwrap();
        let mut paint = Paint::default();
        paint.set_anti_alias(true).set_color(color(WALL_INK_RGB));
        canvas.draw_image_rect(&layer.image, None, Rect::from_wh(width, height), &paint);
        let (x, y) = layer.grid.origin(wall_cursor(frame, layer.grid.tiles));
        let (w, h) = (self.tier.tile_width, self.tier.tile_height);
        let stroke = WALL_CURSOR_WIDTH.min(w).min(h);
        let inset = stroke / 2.0;
        paint.set_style(PaintStyle::Stroke).set_stroke_width(stroke);
        canvas.draw_round_rect(
            Rect::from_xywh(x + inset, y + inset, w - stroke, h - stroke),
            (WALL_CORNER - inset).max(0.0),
            (WALL_CORNER - inset).max(0.0),
            &paint,
        );
    }
}
