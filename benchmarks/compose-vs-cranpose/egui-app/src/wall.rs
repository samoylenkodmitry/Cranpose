//! The still wall in egui (`../README.md`): a window of tiles that never
//! change, under a clock line whose text changes every frame, with a cursor
//! outline that rests on another tile every frame.
//!
//! egui paints the whole screen every frame, as immediate mode does. The
//! tiles' text never changes, so the wall lays it out once, when it first
//! knows how many tiles fit, and paints those galleys each frame: what egui
//! is left to do each frame is to paint every tile.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Galley, Rect, Stroke, StrokeKind, Ui, pos2,
    text::LayoutJob, vec2,
};
use perf_data::*;

use super::{TOP_BAR, bold, roboto};

const fn rgb(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

const BACKGROUND: Color32 = rgb(WALL_BACKGROUND_RGB);
const INK: Color32 = rgb(WALL_INK_RGB);

/// The still wall's state: its frame and the tiles' text, laid out once.
pub struct Wall {
    load: Launch,
    tier: WallTier,
    frame: u32,
    frozen: bool,
    first_frame_logged: bool,
    // The tiles across and down that the text below was laid out for.
    grid: (usize, usize),
    // Each tile's label and value.
    text: Vec<[Arc<Galley>; 2]>,
}

impl Wall {
    pub fn new(ctx: &egui::Context, load: Launch) -> Self {
        ctx.set_fonts(roboto());
        Self {
            load,
            tier: wall_tier(load.tier),
            frame: 0,
            frozen: false,
            first_frame_logged: false,
            grid: (0, 0),
            text: Vec::new(),
        }
    }

    // Lays out every tile's text for a wall `grid` tiles across and down.
    fn lay_out_tiles(&mut self, ui: &Ui, grid: (usize, usize)) {
        let (label_size, value_size) = wall_text_size(self.tier.tile_height);
        let line = |text: String, size: f32, family: FontFamily| {
            let job = LayoutJob::simple_singleline(text, FontId::new(size, family), INK);
            ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
        };
        self.text = (0..grid.0 * grid.1)
            .map(|index| {
                [
                    line(wall_label(index), label_size, bold()),
                    line(wall_value(index), value_size, FontFamily::Proportional),
                ]
            })
            .collect();
        self.grid = grid;
    }

    fn top_bar(ui: &Ui, screen: Rect) -> Rect {
        let rect = Rect::from_min_size(screen.min, vec2(screen.width(), 56.0));
        ui.painter().rect_filled(rect, 0.0, TOP_BAR);
        let title = LayoutJob::simple_singleline(
            "Still wall".to_owned(),
            FontId::new(20.0, bold()),
            Color32::WHITE,
        );
        let title = ui.ctx().fonts_mut(|fonts| fonts.layout_job(title));
        let at = pos2(rect.left() + 16.0, rect.center().y - title.size().y / 2.0);
        ui.painter().galley(at, title, Color32::WHITE);
        rect
    }

    fn clock(&self, ui: &Ui, rect: Rect) {
        let job = LayoutJob::simple_singleline(
            wall_clock(self.frame),
            FontId::new(WALL_CLOCK_TEXT, FontFamily::Proportional),
            INK,
        );
        let clock = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
        let at = pos2(rect.left() + 8.0, rect.center().y - clock.size().y / 2.0);
        ui.painter().galley(at, clock, INK);
    }

    fn wall(&self, ui: &Ui, area: Rect) {
        let painter = ui.painter();
        let (width, height) = (self.tier.tile_width, self.tier.tile_height);
        let padding = wall_padding(height);
        let dot = wall_dot_size(height);
        let corner = CornerRadius::same(WALL_CORNER as u8);
        let tile_rect = |index: usize| {
            let (column, row) = (index % self.grid.0, index / self.grid.0);
            Rect::from_min_size(
                area.min
                    + vec2(
                        WALL_GAP + column as f32 * (width + WALL_GAP),
                        WALL_GAP + row as f32 * (height + WALL_GAP),
                    ),
                vec2(width, height),
            )
        };
        for (index, [label, value]) in self.text.iter().enumerate() {
            let rect = tile_rect(index);
            painter.rect_filled(rect, corner, rgb(wall_fill_rgb(index)));
            let inner = rect.shrink(padding);
            painter.galley(inner.min, label.clone(), INK);
            let line = inner.min.y + label.size().y;
            let center = pos2(inner.min.x + dot / 2.0, line + value.size().y / 2.0);
            painter.circle_filled(center, dot / 2.0, rgb(wall_dot_rgb(index)));
            painter.galley(pos2(inner.min.x + dot * 1.5, line), value.clone(), INK);
        }
        let cursor = tile_rect(wall_cursor(self.frame, self.text.len()));
        painter.rect_stroke(
            cursor,
            corner,
            Stroke::new(WALL_CURSOR_WIDTH, INK),
            StrokeKind::Inside,
        );
    }
}

impl eframe::App for Wall {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if !self.frozen {
            self.frame += 1;
        }
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, BACKGROUND);
        let bar = Self::top_bar(ui, screen);
        let clock = Rect::from_min_size(bar.left_bottom(), vec2(screen.width(), WALL_CLOCK_HEIGHT));
        self.clock(ui, clock);
        let area = Rect::from_min_max(clock.left_bottom(), screen.max);
        let grid = wall_grid(self.tier, area.width(), area.height());
        if grid != self.grid {
            self.lay_out_tiles(ui, grid);
        }
        self.wall(ui, area);
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            log::info!("PERF first_frame");
        }
        if self.frozen {
            return;
        }
        if self.load.freeze > 0 && self.frame >= self.load.freeze {
            self.frozen = true;
            log::info!("PERF frozen frame={}", self.frame);
        } else {
            ui.ctx().request_repaint();
        }
    }
}
