use perf_data::*;

use super::{Box2, Font, Painter, WHITE, chat::Chat};

pub(crate) struct Patchwork {
    tier: WallTier,
    chat: Chat,
}
impl Patchwork {
    pub fn new(tier: usize) -> Self {
        Self {
            tier: wall_tier(tier),
            chat: Chat::new(),
        }
    }
    pub fn advance(&mut self) {
        self.chat.advance();
    }
    pub fn draw(&self, p: &mut impl Painter, body: Box2, k: u32) {
        let w = ((body.w - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
        let h = (body.h / PATCH_ROWS as f32).floor();
        let tiles = PatchTiles::new(self.tier, w, h);
        for (i, patch) in Patch::ALL.into_iter().enumerate() {
            let area = body.at(
                (i % PATCH_COLUMNS) as f32 * w,
                (i / PATCH_COLUMNS) as f32 * h,
                w,
                h,
            );
            p.clipped(area, |p| {
                let panel = Panel { area, tiles };
                if matches!(patch, Patch::Still | Patch::Charts) {
                    p.cached(i, area, |p| panel.draw(p, patch, k));
                } else {
                    panel.draw(p, patch, k);
                }
            });
        }
        self.chat.draw(
            p,
            body.at(w * PATCH_COLUMNS as f32, 0., CHAT_WIDTH, body.h),
            k,
        );
    }
}

#[derive(Clone, Copy)]
struct Panel {
    area: Box2,
    tiles: PatchTiles,
}
impl Panel {
    fn cell(self, i: usize, columns: usize, w: f32, h: f32) -> Box2 {
        self.area.at(
            WALL_GAP + (i % columns) as f32 * (w + WALL_GAP),
            WALL_GAP + (i / columns) as f32 * (h + WALL_GAP),
            w,
            h,
        )
    }
    fn tiles(self, p: &mut impl Painter, patch: Patch, k: u32) {
        let t = self.tiles;
        let area = self.area;
        let total = patchwork_scroll_rows(t.columns);
        let first = if patch == Patch::Scroll {
            patchwork_scroll_first(k, total, t.rows)
        } else {
            0
        };
        let count = if patch == Patch::Scroll {
            (((area.h - WALL_GAP) / (t.height + WALL_GAP)).ceil() as usize).min(total - first)
                * t.columns
        } else {
            t.count()
        };
        for i in 0..count {
            let index = first * t.columns + i;
            let fill = if patch == Patch::Recolor {
                patchwork_recolor_rgb(i, k, t.in_turn())
            } else {
                wall_fill_rgb(index)
            };
            tile(
                p,
                self.cell(i, t.columns, t.width, t.height),
                index,
                t.height,
                fill,
                255,
            );
        }
    }
    fn cards(self, p: &mut impl Painter, k: u32) {
        let t = self.tiles;
        let area = self.area;
        let cards = Cards::new(t.columns, t.rows, t.height, area.h);
        for position in 0..cards.places() {
            let Some(card) = cards.place(position, k) else {
                continue;
            };
            let header = area.at(
                WALL_GAP,
                card.top,
                patchwork_reorder_width(cards.columns, t.width),
                t.height,
            );
            p.rect(header, patchwork_card_rgb(card.id), 255, WALL_CORNER);
            p.text(
                header.inset(wall_padding(t.height)),
                &patchwork_card_title(card.id),
                Font::bold(wall_text_size(t.height).0, WHITE),
                false,
            );
            for i in 0..cards.tiles() {
                let index = patchwork_card_tile(card.id, i, cards.tiles());
                let mut b = self.cell(i, cards.columns, t.width, t.height);
                b.y += card.top + t.height;
                if b.y + b.h > area.y && b.y < area.y + area.h {
                    tile(p, b, index, t.height, wall_fill_rgb(index), 255);
                }
            }
        }
    }
    fn reorder(self, p: &mut impl Painter, k: u32) {
        let t = self.tiles;
        let area = self.area;
        let count = patchwork_reorder_count(t.count());
        let (mut x, mut y) = (WALL_GAP, WALL_GAP);
        for pos in 0..count {
            let i = patchwork_reorder_item(pos, k, count);
            let w = patchwork_reorder_width(patchwork_reorder_units(i), t.width);
            if x > WALL_GAP && x + w > area.w - WALL_GAP {
                x = WALL_GAP;
                y += t.height + WALL_GAP;
            }
            if y >= area.h {
                break;
            }
            tile(
                p,
                area.at(x, y, w, t.height),
                i,
                t.height,
                wall_fill_rgb(i),
                255,
            );
            x += w + WALL_GAP;
        }
    }

    fn draw(self, p: &mut impl Painter, patch: Patch, k: u32) {
        let t = self.tiles;
        let area = self.area;
        let (dw, dh) = (patchwork_double(t.width), patchwork_double(t.height));
        let (pairs, rows) = patchwork_pairs(t.columns, t.rows);
        match patch {
            Patch::Still | Patch::Recolor | Patch::Scroll => self.tiles(p, patch, k),
            Patch::Charts => {
                let mut line = Vec::with_capacity(PATCHWORK_CHART_POINTS);
                for i in 0..pairs * rows {
                    let b = self.cell(i, pairs, dw, dh);
                    line.clear();
                    line.extend((0..PATCHWORK_CHART_POINTS).map(|pt| {
                        (
                            b.x + b.w * pt as f32 / (PATCHWORK_CHART_POINTS - 1) as f32,
                            b.y + b.h * (1. - patchwork_chart_value(i, pt)),
                        )
                    }));
                    p.path(&line, patchwork_chart_rgb(i), PATCHWORK_CHART_STROKE, None);
                }
            }
            Patch::Deep => {
                for i in 0..pairs * rows {
                    let mut b = self.cell(i, pairs, dw, dh);
                    for level in 0..PATCHWORK_DEPTH {
                        p.rect(b, patchwork_deep_rgb(i, level), 255, 0.);
                        b = b.inset(wall_padding(t.height));
                    }
                    p.text(
                        b,
                        &patchwork_deep_value(
                            i,
                            k,
                            InTurn {
                                columns: pairs,
                                rows,
                            },
                        ),
                        Font::bold(wall_text_size(t.height).0, WHITE),
                        false,
                    );
                }
            }
            Patch::Cards => self.cards(p, k),
            Patch::Reorder => self.reorder(p, k),
            Patch::Paragraph => p.text(
                area.inset(WALL_GAP),
                &patchwork_paragraph(k, patchwork_paragraph_words(t.count())),
                Font::regular(wall_text_size(t.height).0, WHITE),
                true,
            ),
            Patch::Pile => {
                let (columns, rows) = patchwork_pile_grid(t.columns, t.rows);
                let opaque = patchwork_pile_opaque(k, columns * rows);
                for i in 0..columns * rows {
                    let mut b = self.cell(i, columns, t.width, t.height);
                    b.w = dw;
                    b.h = dh;
                    let (fill, alpha) = if i == opaque {
                        (PATCHWORK_PILE_OPAQUE_RGB, 255)
                    } else {
                        (wall_fill_rgb(i), PATCHWORK_PILE_ALPHA)
                    };
                    tile(p, b, i, t.height, fill, alpha);
                }
            }
        }
    }
}

fn tile(p: &mut impl Painter, b: Box2, i: usize, h: f32, fill: [u8; 3], alpha: u8) {
    let (ls, vs) = wall_text_size(h);
    let dot = wall_dot_size(h);
    p.rect(b, fill, alpha, WALL_CORNER);
    let b = b.inset(wall_padding(h));
    let row = ls * 1.17;
    p.text(
        b.at(0., 0., b.w, row),
        &wall_label(i),
        Font::bold(ls, WHITE),
        false,
    );
    p.rect(
        b.at(0., row + (vs * 1.17 - dot) * 0.5, dot, dot),
        wall_dot_rgb(i),
        255,
        dot / 2.,
    );
    p.text(
        b.at(dot * 1.5, row, b.w - dot * 1.5, b.h - row),
        &wall_value(i),
        Font::regular(vs, WHITE),
        false,
    );
}
