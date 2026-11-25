use perf_data::*;

use super::{Box2, Font, ImageId, Painter, WHITE};

const INK: [u8; 3] = GAUNTLET_INK_RGB;
const BODY: [u8; 3] = GAUNTLET_BODY_RGB;
const MUTED: [u8; 3] = GAUNTLET_MUTED_RGB;

pub(crate) struct Gauntlet {
    anchor: ListAnchor,
}
impl Gauntlet {
    pub fn new() -> Self {
        Self {
            anchor: ListAnchor {
                row: 0,
                top: 8. * desktop_gauntlet().tier.scale,
            },
        }
    }
    pub fn draw(&mut self, p: &mut impl Painter, body: Box2, k: u32) {
        let load = desktop_gauntlet();
        let tier = load.tier;
        let s = tier.scale;
        let body = Box2 {
            w: (body.w * width_fraction(k)).round(),
            ..body
        };
        let strip = tickers(p, body, k);
        let list = body.at(0., strip, body.w, body.h - strip);
        let offset = k as f32 * SCROLL_PER_FRAME;
        p.clipped(list, |p| {
            let mut y = self.anchor.top - offset;
            for row in self.anchor.row..GAUNTLET_ROWS {
                if y >= list.h {
                    break;
                }
                let b = list.at(8. * s, y, list.w - 16. * s, list.h);
                let h = match gauntlet_row(row, tier.columns) {
                    GauntletRow::Cluster(id) => cluster(p, b, id, tier.depth, s),
                    GauntletRow::Cards(first) => {
                        let w = (b.w - 8. * s * (tier.columns - 1) as f32) / tier.columns as f32;
                        (0..tier.columns)
                            .map(|i| {
                                card(
                                    p,
                                    b.at(i as f32 * (w + 8. * s), 0., w, b.h),
                                    first + i,
                                    k,
                                    s,
                                )
                            })
                            .fold(0., f32::max)
                    }
                };
                y += h + 8. * s;
                if y <= 0. {
                    self.anchor = ListAnchor {
                        row: row + 1,
                        top: offset + y,
                    };
                }
            }
            for i in 0..tier.layers {
                layer(p, list, i, k, tier.layer_rows);
            }
        });
    }
}

fn tickers(p: &mut impl Painter, area: Box2, k: u32) -> f32 {
    let load = desktop_gauntlet();
    let s = load.tier.scale;
    let pad = 6. * s;
    let mut tiles = Vec::with_capacity(load.tickers.len());
    let normal = Font::regular(10. * s, BODY);
    let bold = Font::bold(10. * s, INK);
    let (mut x, mut y) = (pad, pad);
    let h = p.size("0", normal, None).1 + 6. * s;
    for t in &load.tickers {
        let cents = ticker_cents(t, k);
        let price = cents_text(cents);
        let change = change_text(t, cents);
        let widths = [
            p.size(&t.symbol, bold, None).0,
            p.size(&price, normal, None).0,
            p.size(&change, normal, None).0,
        ];
        let w = widths.iter().sum::<f32>() + (12. + 12. + 20.) * s;
        if x > pad && x + w > area.w - pad {
            x = pad;
            y += h + 4. * s;
        }
        tiles.push((area.at(x, y, w, h), price, change, cents, widths));
        x += w + 4. * s;
    }
    let total = y + h + pad;
    p.rect(area.at(0., 0., area.w, total), GAUNTLET_PANEL_RGB, 255, 0.);
    for (i, (b, price, change, cents, widths)) in tiles.into_iter().enumerate() {
        let t = &load.tickers[i];
        p.rect(b, WHITE, 255, 6. * s);
        let up = if cents >= t.base_cents {
            GAUNTLET_UP_RGB
        } else {
            GAUNTLET_DOWN_RGB
        };
        let mut x = 6. * s;
        for ((value, font), w) in [
            (&t.symbol, bold),
            (&price, normal),
            (
                &change,
                Font {
                    color: up,
                    ..normal
                },
            ),
        ]
        .into_iter()
        .zip(widths)
        {
            p.text(b.at(x, 3. * s, w + 0.1, h - 6. * s), value, font, false);
            x += w + 4. * s;
        }
        let share = (cents - t.base_cents + t.swing_cents) as f32 / (2 * t.swing_cents) as f32;
        bar(p, b.at(x, (h - 4. * s) * 0.5, 20. * s, 4. * s), i, share);
    }
    total
}

fn bar(p: &mut impl Painter, b: Box2, i: usize, share: f32) {
    p.rect(b, GAUNTLET_HAIRLINE_RGB, 255, b.h / 2.);
    p.rect(
        Box2 {
            w: b.w * share.clamp(0., 1.),
            ..b
        },
        PALETTE_RGB[i % PALETTE_RGB.len()],
        255,
        b.h / 2.,
    );
}

fn card(p: &mut impl Painter, b: Box2, id: usize, k: u32, s: f32) -> f32 {
    let post = &desktop_gauntlet().posts[id % POST_COUNT];
    let pad = 10. * s;
    let gap = 6. * s;
    let inner = b.inset(pad);
    let w = inner.w;
    let title = Font::bold(13. * s, INK);
    let body = Font::regular(12. * s, BODY);
    let subtitle = Font::regular(11. * s, MUTED);
    let small = Font::regular(10. * s, MUTED);
    let title_w = (w - 40. * s).max(1.);
    let title_h = p
        .size(&post.title, title, Some(title_w))
        .1
        .min(2. * p.size("M", title, None).1);
    let sub_h = p.size("M", subtitle, None).1;
    let header_h = (title_h + sub_h).max(32. * s);
    let body_h = p
        .size(&post.body, body, Some(w))
        .1
        .min(4. * p.size("M", body, None).1);
    let progress_h = p.size("0%", small, None).1;
    let mut chips = Vec::with_capacity(post.tags.len());
    let mut cx = 0.;
    let mut cy = 0.;
    let chip_h = progress_h + 6. * s;
    for (name, color) in &post.tags {
        let width = p.size(name, small, None).0 + 16. * s;
        if cx > 0. && cx + width > w {
            cx = 0.;
            cy += chip_h + 4. * s;
        }
        chips.push((cx, cy, width, name, color));
        cx += width + 4. * s;
    }
    let footer_h = p.size("0", Font::bold(12. * s, INK), None).1
        + p.size("likes", Font::regular(9. * s, MUTED), None).1;
    let height =
        2. * pad + 5. * gap + header_h + body_h + progress_h + 36. * s + cy + chip_h + footer_h;
    let b = Box2 { h: height, ..b };
    p.rect(b.at(0., s, b.w, b.h + 2. * s), [0; 3], 35, 12. * s);
    p.rect(b, GAUNTLET_HAIRLINE_RGB, 255, 12. * s);
    p.rect(b.inset(1.), WHITE, 255, (12. * s - 1.).max(0.));
    p.image(
        inner.at(0., (header_h - 32. * s) / 2., 32. * s, 32. * s),
        ImageId::Avatar(id),
        16. * s,
    );
    let ty = (header_h - title_h - sub_h) / 2.;
    p.text(
        inner.at(40. * s, ty, title_w, title_h),
        &post.title,
        title,
        true,
    );
    subtitle_text(
        p,
        inner.at(40. * s, ty + title_h, title_w, sub_h),
        post,
        subtitle,
    );
    let mut y = header_h + gap;
    p.text(inner.at(0., y, w, body_h), &post.body, body, true);
    y += body_h + gap;
    progress(p, inner.at(0., y, w, progress_h), id, k, s, small);
    y += progress_h + gap;
    sparkline(p, inner.at(0., y, w, 36. * s), id, k, s, post.color);
    y += 36. * s + gap;
    for (x, dy, width, name, color) in chips {
        let chip = inner.at(x, y + dy, width, chip_h);
        p.rect(chip, CHIP_BACKGROUND_RGB[*color], 255, 10. * s);
        p.text(
            chip.at(8. * s, 3. * s, width - 16. * s + 0.1, progress_h),
            name,
            Font {
                color: GRADIENT_END_RGB[*color],
                ..small
            },
            false,
        );
    }
    y += cy + chip_h + gap;
    footer(p, inner.at(0., y, w, footer_h), post, s);
    if id.is_multiple_of(5) {
        let f = Font::bold(9. * s, WHITE);
        let (tw, th) = p.size("HOT", f, None);
        let badge = b.at(b.w - tw - 18. * s, 6. * s, tw + 12. * s, th + 4. * s);
        p.rotated(badge.center(), badge_degrees(id, k), |p| {
            p.rect(badge, PALETTE_RGB[0], 230, 8. * s);
            p.text(badge.at(6. * s, 2. * s, tw + 0.1, th), "HOT", f, false);
        });
    }
    height
}

fn subtitle_text(p: &mut impl Painter, sub: Box2, post: &Post, subtitle: Font) {
    let s = subtitle.size / 11.;
    let (tag, tag_color) = &post.tags[0];
    p.clipped(sub, |p| {
        let mut x = 0.;
        for (word, font) in [
            ("by ", subtitle),
            (post.author.as_str(), Font::bold(11. * s, MUTED)),
            (" · ", subtitle),
            (
                tag.as_str(),
                Font {
                    color: GRADIENT_END_RGB[*tag_color],
                    ..subtitle
                },
            ),
        ] {
            let (width, _) = p.size(word, font, None);
            p.text(sub.at(x, 0., width + 0.1, sub.h), word, font, false);
            x += width;
        }
    });
}

fn progress(p: &mut impl Painter, inner: Box2, id: usize, k: u32, s: f32, small: Font) {
    let (w, y, gap, progress_h) = (inner.w, 0., 6. * s, inner.h);
    let permille = progress_permille(id, k);
    let percent = format!("{}%", permille / 10);
    let tw = p.size(&percent, small, None).0;
    bar(
        p,
        inner.at(0., y + (progress_h - 6. * s) / 2., w - tw - gap, 6. * s),
        id,
        permille as f32 / 1000.,
    );
    p.text(
        inner.at(w - tw, y, tw + 0.1, progress_h),
        &percent,
        small,
        false,
    );
}

fn sparkline(p: &mut impl Painter, inner: Box2, id: usize, k: u32, s: f32, color: usize) {
    let (y, w) = (0., inner.w);
    let points: [_; GAUNTLET_SPARK_POINTS] = std::array::from_fn(|pt| {
        (
            inner.x + w * pt as f32 / (GAUNTLET_SPARK_POINTS - 1) as f32,
            inner.y + y + 36. * s * (1. - spark_value(id, pt, k)),
        )
    });
    p.path(
        &points,
        PALETTE_RGB[color],
        1.5 * s,
        Some(inner.y + y + 36. * s),
    );
}

fn footer(p: &mut impl Painter, inner: Box2, post: &Post, s: f32) {
    let (w, y, footer_h) = (inner.w, 0., inner.h);
    for (i, name) in ["likes", "replies", "shares"].into_iter().enumerate() {
        let col = inner.at(i as f32 * w / 3., y, w / 3., footer_h);
        if i > 0 {
            p.rect(col.at(0., 0., 1., footer_h), GAUNTLET_HAIRLINE_RGB, 255, 0.);
        }
        let stat = &post.stats[i];
        let font = Font::bold(12. * s, INK);
        let (tw, th) = p.size(stat, font, None);
        p.text(
            col.at((col.w - tw) / 2., 0., tw + 0.1, th),
            stat,
            font,
            false,
        );
        let font = Font::regular(9. * s, MUTED);
        let (tw, h) = p.size(name, font, None);
        p.text(
            col.at((col.w - tw) / 2., th, tw + 0.1, h),
            name,
            font,
            false,
        );
    }
}

fn cluster(p: &mut impl Painter, mut b: Box2, id: usize, depth: usize, s: f32) -> f32 {
    let font = Font::regular(9. * s, WHITE);
    let line = p.size("M", font, None).1;
    let height = (depth + 1) as f32 * (line + 3. * s) - s;
    b.h = height;
    for level in (0..=depth).rev() {
        p.rect(b, LEVEL_BACKGROUND_RGB[level % 2], 255, 4. * s);
        let w = (b.w - 8. * s) / 3.;
        for i in 0..3 {
            let chip = b.at(3. * s + i as f32 * (w + 2. * s), s, w, line);
            p.rect(chip, PALETTE_RGB[(level + i + id) % 8], 255, 2. * s);
            p.text(chip, &format!("C{id}.L{level}.{i}"), font, false);
        }
        b = b.at(3. * s, line + 2. * s, b.w - 4. * s, b.h - line - 3. * s);
    }
    height
}

fn layer(p: &mut impl Painter, list: Box2, i: usize, k: u32, rows: usize) {
    let title = Font::bold(13., WHITE);
    let caption = Font::regular(11., BODY);
    let head = p.size("Layer", title, None).1;
    let line = p.size("Nested", caption, None).1;
    let nest_h = 18. + 2. * line;
    let cells_h = rows as f32 * 25. - 3.;
    let panel = list.at(
        layer_x(i, k),
        layer_y(i, k),
        220.,
        20. + head + 16. + cells_h + nest_h,
    );
    let angle = layer_degrees(i, k);
    p.rotated(panel.center(), angle, |p| {
        p.rect(panel, [30, 41, 59], 192, 12.);
        let inner = panel.inset(10.);
        p.text(
            inner.at(0., 0., inner.w, head),
            &format!("Layer {}", i + 1),
            title,
            false,
        );
        let top = head + 8.;
        for index in 0..rows * LAYER_COLUMNS {
            let b = inner.at(
                (index % LAYER_COLUMNS) as f32 * 25.,
                top + (index / LAYER_COLUMNS) as f32 * 25.,
                22.,
                22.,
            );
            p.rect(b, PALETTE_RGB[layer_cell_color(i, index)], 255, 4.);
            let f = Font::regular(9., WHITE);
            let n = (index + 1).to_string();
            let (tw, th) = p.size(&n, f, None);
            p.text(
                b.at((22. - tw) / 2., (22. - th) / 2., tw + 0.1, th),
                &n,
                f,
                false,
            );
        }
        let nest = inner.at(0., top + cells_h + 8., inner.w, nest_h);
        p.rotated(nest.center(), -angle, |p| {
            p.rect(nest, WHITE, 230, 8.);
            let n = nest.inset(8.);
            p.text(
                n.at(0., 0., n.w, line),
                &format!("Nested in layer {}", i + 1),
                Font::bold(11., INK),
                false,
            );
            p.text(
                n.at(0., line + 2., n.w, line),
                "Tilts against its panel",
                caption,
                false,
            );
        });
    });
}
