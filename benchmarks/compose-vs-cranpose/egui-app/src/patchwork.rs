//! The patchwork in egui (`../README.md`): nine patches of the still wall's
//! tiles, each of which changes in a way of its own every frame.
//!
//! egui paints every patch every frame. The text of the tiles that never
//! change is laid out once, as on the still wall; the text a frame changes
//! (the nests' digits, the list's rows and the paragraph) is laid out in the
//! frame that shows it.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Galley, Painter, Rect, TextFormat, Ui, pos2,
    text::{LayoutJob, TextWrapping},
    vec2,
};
use perf_data::*;

use super::{
    bold,
    wall::{BACKGROUND, INK, line, paint_clock, paint_tile, rgb, tile_text, top_bar},
};

/// The patchwork's state: its frame and the still tiles' text, laid out once.
pub struct Patchwork {
    tier: WallTier,
    frame: FrameIndex,
    // The tiles across and down a patch that the text below was laid out for.
    grid: (usize, usize),
    // The label and the value of each tile of a patch.
    text: Vec<[Arc<Galley>; 2]>,
}

// One patch in a frame: where it lies and what the frame paints in it.
struct Shown<'a> {
    ui: &'a Ui,
    painter: Painter,
    rect: Rect,
    frame: u32,
}

impl Patchwork {
    pub fn new(ctx: &egui::Context, load: Launch) -> Self {
        ctx.set_fonts(super::roboto());
        Self {
            tier: wall_tier(load.tier),
            frame: FrameIndex::new(load.freeze),
            grid: (0, 0),
            text: Vec::new(),
        }
    }

    fn tiles(&self) -> usize {
        self.grid.0 * self.grid.1
    }

    // Where the tile at `column` and `row` of a patch at `origin` lies,
    // `across` by `down` tiles large.
    fn cell(
        &self,
        origin: egui::Pos2,
        column: usize,
        row: usize,
        across: usize,
        down: usize,
    ) -> Rect {
        let (width, height) = (self.tier.tile_width, self.tier.tile_height);
        Rect::from_min_size(
            origin
                + vec2(
                    WALL_GAP + column as f32 * (width + WALL_GAP),
                    WALL_GAP + row as f32 * (height + WALL_GAP),
                ),
            vec2(
                patchwork_reorder_width(across, width),
                patchwork_reorder_width(down, height),
            ),
        )
    }

    fn tile(&self, origin: egui::Pos2, index: usize) -> Rect {
        self.cell(origin, index % self.grid.0, index / self.grid.0, 1, 1)
    }

    fn still(&self, shown: &Shown) {
        for (index, text) in self.text.iter().enumerate() {
            let rect = self.tile(shown.rect.min, index);
            let fill = rgb(wall_fill_rgb(index));
            paint_tile(
                &shown.painter,
                rect,
                fill,
                self.tier.tile_height,
                index,
                text,
            );
        }
    }

    fn charts(&self, shown: &Shown) {
        let (columns, rows) = patchwork_pairs(self.grid.0, self.grid.1);
        let (width, height) = (
            patchwork_double(self.tier.tile_width),
            patchwork_double(self.tier.tile_height),
        );
        let step = width / (PATCHWORK_CHART_POINTS - 1) as f32;
        for index in 0..columns * rows {
            let min = shown.rect.min
                + vec2(
                    WALL_GAP + (index % columns) as f32 * (width + WALL_GAP),
                    WALL_GAP + (index / columns) as f32 * (height + WALL_GAP),
                );
            let points = (0..PATCHWORK_CHART_POINTS)
                .map(|point| {
                    min + vec2(
                        point as f32 * step,
                        height * (1.0 - patchwork_chart_value(index, point)),
                    )
                })
                .collect();
            shown.painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(PATCHWORK_CHART_STROKE, rgb(patchwork_chart_rgb(index))),
            ));
        }
    }

    fn recolor(&self, shown: &Shown) {
        for (index, text) in self.text.iter().enumerate() {
            let rect = self.tile(shown.rect.min, index);
            let fill = rgb(patchwork_recolor_rgb(index, shown.frame));
            paint_tile(
                &shown.painter,
                rect,
                fill,
                self.tier.tile_height,
                index,
                text,
            );
        }
    }

    fn churn(&self, shown: &Shown) {
        let height = self.tier.tile_height;
        let corner = CornerRadius::same(WALL_CORNER as u8);
        for (index, text) in self.text.iter().enumerate() {
            let rect = self.tile(shown.rect.min, index);
            let fill = rgb(wall_fill_rgb(index));
            match patchwork_churn(index, shown.frame) {
                ChurnKind::Tile => paint_tile(&shown.painter, rect, fill, height, index, text),
                ChurnKind::Blank => {
                    shown
                        .painter
                        .rect_filled(rect, corner, rgb(wall_dot_rgb(index)));
                }
                ChurnKind::Label => {
                    shown.painter.rect_filled(rect, corner, fill);
                    let at = rect.shrink(wall_padding(height)).min;
                    shown.painter.galley(at, text[0].clone(), INK);
                }
            }
        }
    }

    // The items in the frame's order, each on the row's next free tiles or
    // else at the start of the next row.
    fn reorder(&self, shown: &Shown) {
        let count = patchwork_reorder_count(self.tiles());
        let (mut column, mut row) = (0, 0);
        for position in 0..count {
            let item = patchwork_reorder_item(position, shown.frame, count);
            let units = patchwork_reorder_units(item);
            if column + units > self.grid.0 && column > 0 {
                (column, row) = (0, row + 1);
            }
            let rect = self.cell(shown.rect.min, column, row, units, 1);
            let fill = rgb(wall_fill_rgb(item));
            paint_tile(
                &shown.painter,
                rect,
                fill,
                self.tier.tile_height,
                item,
                &self.text[item],
            );
            column += units;
        }
    }

    fn deep(&self, shown: &Shown) {
        let (columns, rows) = patchwork_pairs(self.grid.0, self.grid.1);
        let (width, height) = (
            patchwork_double(self.tier.tile_width),
            patchwork_double(self.tier.tile_height),
        );
        let padding = wall_padding(self.tier.tile_height);
        let size = wall_text_size(self.tier.tile_height).0;
        for index in 0..columns * rows {
            let (column, row) = (index % columns, index / columns);
            let mut rect = Rect::from_min_size(
                shown.rect.min
                    + vec2(
                        WALL_GAP + column as f32 * (width + WALL_GAP),
                        WALL_GAP + row as f32 * (height + WALL_GAP),
                    ),
                vec2(width, height),
            );
            for level in 0..PATCHWORK_DEPTH {
                shown
                    .painter
                    .rect_filled(rect, 0.0, rgb(patchwork_deep_rgb(index, level)));
                rect = rect.shrink(padding);
            }
            let value = line(
                shown.ui,
                patchwork_deep_value(index, shown.frame),
                size,
                bold(),
            );
            shown.painter.galley(rect.min, value, INK);
        }
    }

    // The rows the frame shows, and the one its edge cuts: the list lays out
    // the text of each, as no frame shows the rows of the one before.
    fn jump(&self, shown: &Shown) {
        let (columns, rows) = self.grid;
        let first = patchwork_jump_first(shown.frame, patchwork_jump_rows(columns), rows);
        for row in 0..=rows {
            for column in 0..columns {
                let index = (first + row) * columns + column;
                let text = tile_text(shown.ui, self.tier.tile_height, index);
                paint_tile(
                    &shown.painter,
                    self.cell(shown.rect.min, column, row, 1, 1),
                    rgb(wall_fill_rgb(index)),
                    self.tier.tile_height,
                    index,
                    &text,
                );
            }
        }
    }

    fn paragraph(&self, shown: &Shown) {
        let size = wall_text_size(self.tier.tile_height).0;
        let mut job = LayoutJob::single_section(
            patchwork_paragraph(shown.frame, patchwork_paragraph_words(self.tiles())),
            TextFormat {
                font_id: FontId::new(size, FontFamily::Proportional),
                color: INK,
                line_height: Some(size * 1.4),
                ..Default::default()
            },
        );
        job.wrap = TextWrapping {
            max_width: shown.rect.width() - 2.0 * WALL_GAP,
            ..Default::default()
        };
        let text = shown.ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
        shown
            .painter
            .galley(shown.rect.min + vec2(WALL_GAP, WALL_GAP), text, INK);
    }

    fn pile(&self, shown: &Shown) {
        let (columns, rows) = patchwork_pile_grid(self.grid.0, self.grid.1);
        let opaque = patchwork_pile_opaque(shown.frame, columns * rows);
        for index in 0..columns * rows {
            let fill = if index == opaque {
                rgb(PATCHWORK_PILE_OPAQUE_RGB)
            } else {
                let [red, green, blue] = wall_fill_rgb(index);
                Color32::from_rgba_unmultiplied(red, green, blue, PATCHWORK_PILE_ALPHA)
            };
            paint_tile(
                &shown.painter,
                self.cell(shown.rect.min, index % columns, index / columns, 2, 2),
                fill,
                self.tier.tile_height,
                index,
                &self.text[index],
            );
        }
    }
}

impl eframe::App for Patchwork {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let more = self.frame.advance();
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, BACKGROUND);
        let bar = top_bar(ui, screen, "Patchwork");
        let clock = Rect::from_min_size(bar.left_bottom(), vec2(screen.width(), WALL_CLOCK_HEIGHT));
        paint_clock(ui, clock, self.frame.get());
        let area = Rect::from_min_max(clock.left_bottom(), screen.max);
        let patch = vec2(
            ((area.width() - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor(),
            (area.height() / PATCH_ROWS as f32).floor(),
        );
        let grid = wall_grid(self.tier, patch.x, patch.y);
        if grid != self.grid {
            self.grid = grid;
            self.text = (0..grid.0 * grid.1)
                .map(|index| tile_text(ui, self.tier.tile_height, index))
                .collect();
        }
        for (place, kind) in Patch::ALL.into_iter().enumerate() {
            let min = pos2(
                area.left() + (place % PATCH_COLUMNS) as f32 * patch.x,
                area.top() + (place / PATCH_COLUMNS) as f32 * patch.y,
            );
            let rect = Rect::from_min_size(min, patch);
            let shown = Shown {
                ui,
                painter: ui.painter().with_clip_rect(rect),
                rect,
                frame: self.frame.get(),
            };
            match kind {
                Patch::Still => self.still(&shown),
                Patch::Charts => self.charts(&shown),
                Patch::Recolor => self.recolor(&shown),
                Patch::Churn => self.churn(&shown),
                Patch::Reorder => self.reorder(&shown),
                Patch::Deep => self.deep(&shown),
                Patch::Jump => self.jump(&shown),
                Patch::Paragraph => self.paragraph(&shown),
                Patch::Pile => self.pile(&shown),
            }
        }
        if more {
            ui.ctx().request_repaint();
        }
    }
}
