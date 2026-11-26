use perf_data::*;

use super::{Box2, Face, Font, ImageId, Painter, WHITE};

const GAP: f32 = 8.;
pub(super) struct Chat {
    feed: ChatFeed,
    token: Option<ChatToken>,
}
impl Chat {
    pub fn new() -> Self {
        Self {
            feed: ChatFeed::start().expect("chat data thread"),
            token: None,
        }
    }
    pub fn advance(&mut self) {
        if let Some(t) = (0..CHAT_TOKENS_PER_FRAME)
            .map_while(|_| self.feed.take())
            .last()
        {
            self.token = Some(t);
        }
    }
    pub fn draw(&self, p: &mut impl Painter, area: Box2, k: u32) {
        p.rect(area, CHAT_BACKGROUND_RGB, 255, 0.);
        let list = area.at(0., 0., area.w, area.h - CHAT_STATUS_HEIGHT);
        if let Some(token) = self.token {
            p.clipped(list, |p| {
                let content = list.inset(GAP);
                let mut visible = Vec::new();
                let mut height = 0.;
                for index in (0..=token.block).rev() {
                    let block = Block::new(index, (index == token.block).then_some(token.shown));
                    let h = block.height(p, content.w);
                    visible.push((block, h));
                    height += h + GAP;
                    if height - GAP >= content.h {
                        break;
                    }
                }
                let mut y = (content.h - (height - GAP)).min(0.);
                for (block, h) in visible.into_iter().rev() {
                    block.draw(p, content.at(0., y, content.w, h), k);
                    y += h + GAP;
                }
            });
        }
        let status = area.at(0., list.h, area.w, CHAT_STATUS_HEIGHT);
        p.rect(status, CHAT_PANEL_RGB, 255, 0.);
        let spinner = status.at(
            GAP,
            (status.h - CHAT_SPINNER_SIZE) / 2.,
            CHAT_SPINNER_SIZE,
            CHAT_SPINNER_SIZE,
        );
        p.rotated(spinner.center(), k as f32 * CHAT_SPIN_DEGREES, |p| {
            p.image(spinner, ImageId::Spinner, 0.)
        });
        if let Some(token) = self.token {
            let font = Font::regular(CHAT_TEXT, WHITE);
            let text = chat_status(token);
            let mut x = 0.;
            for (i, c) in text.char_indices() {
                let glyph = &text[i..i + c.len_utf8()];
                let (w, h) = p.size(glyph, font, None);
                let [lo, hi] = CHAT_SHIMMER_RGB;
                let weight = chat_shimmer(x, k);
                let color = std::array::from_fn(|i| {
                    (lo[i] as f32 + weight * (hi[i] as f32 - lo[i] as f32)) as u8
                });
                p.text(
                    status.at(
                        GAP + CHAT_SPINNER_SIZE + 6. + x,
                        (status.h - h) / 2.,
                        w + 1.,
                        h,
                    ),
                    glyph,
                    Font { color, ..font },
                    false,
                );
                x += w;
            }
        }
    }
}

enum Block {
    User(&'static str),
    Typed(String, Font),
    Code(String),
    Table(&'static [[&'static str; CHAT_TABLE_COLUMNS]]),
    Gif,
    Buttons(&'static [&'static str]),
}
impl Block {
    fn new(index: usize, shown: Option<usize>) -> Self {
        let block = CHAT_TURN[index % CHAT_TURN.len()];
        let prefix = |s: &'static str| &s[..shown.map_or(s.len(), |n| chat_shown_chars(s, n))];
        let turn = || chat_turn_label(ChatToken::turn(index));
        match block {
            ChatBlock::User(s) => Self::User(s),
            ChatBlock::Heading(s) | ChatBlock::Paragraph(s) => {
                let font = if matches!(block, ChatBlock::Heading(_)) {
                    Font::bold(CHAT_HEADING_TEXT, WHITE)
                } else {
                    Font::regular(CHAT_TEXT, WHITE)
                };
                Self::Typed(format!("{} {}", turn(), prefix(s)), font)
            }
            ChatBlock::Code(s) => Self::Code(format!("// {}\n{}", turn(), prefix(s))),
            ChatBlock::Table(rows) => {
                Self::Table(&rows[..shown.unwrap_or(rows.len()).min(rows.len())])
            }
            ChatBlock::Gif => Self::Gif,
            ChatBlock::Buttons(labels) => Self::Buttons(labels),
        }
    }
    fn height(&self, p: &mut impl Painter, w: f32) -> f32 {
        match self {
            Self::User(s) => p.size(s, Font::regular(CHAT_TEXT, WHITE), Some(w - 20.)).1 + 12.,
            Self::Typed(s, font) => p.size(s, *font, Some(w)).1,
            Self::Code(s) => code_layout(p, None, w, s),
            Self::Table(rows) => rows
                .iter()
                .map(|row| row_height(p, row, (w - 12.) / CHAT_TABLE_COLUMNS as f32))
                .sum(),
            Self::Gif => CHAT_GIF_SIZE,
            Self::Buttons(_) => p.size("Action", Font::bold(CHAT_TEXT, WHITE), None).1 + 12.,
        }
    }
    fn draw(&self, p: &mut impl Painter, b: Box2, k: u32) {
        match self {
            Self::User(s) => {
                let f = Font::regular(CHAT_TEXT, WHITE);
                let w = (p.size(s, f, None).0 + 20.).min(b.w);
                let bubble = b.at(b.w - w, 0., w, b.h);
                p.rect(bubble, CHAT_USER_RGB, 255, 10.);
                p.text(bubble.at(10., 6., w - 20., b.h - 12.), s, f, true);
            }
            Self::Typed(s, f) => p.text(b, s, *f, true),
            Self::Code(s) => {
                p.rect(b, CHAT_PANEL_RGB, 255, 6.);
                code_layout(p, Some(b), b.w, s);
            }
            Self::Table(rows) => {
                let w = (b.w - 12.) / CHAT_TABLE_COLUMNS as f32;
                let mut y = 0.;
                for (i, row) in rows.iter().enumerate() {
                    let h = row_height(p, row, w);
                    if i == 0 || i % 2 == 1 {
                        p.rect(b.at(0., y, b.w, h), CHAT_PANEL_RGB, 255, 0.);
                    }
                    for (col, s) in row.iter().enumerate() {
                        let f = if i == 0 {
                            Font::bold(CHAT_CODE_TEXT, WHITE)
                        } else {
                            Font::regular(CHAT_CODE_TEXT, WHITE)
                        };
                        p.text(b.at(6. + col as f32 * w, y + 3., w, h - 6.), s, f, true);
                    }
                    y += h;
                }
            }
            Self::Gif => p.image(
                b.at(0., 0., CHAT_GIF_SIZE, CHAT_GIF_SIZE),
                ImageId::Gif(k as usize),
                0.,
            ),
            Self::Buttons(labels) => {
                let mut x = 0.;
                let f = Font::bold(CHAT_TEXT, WHITE);
                for s in *labels {
                    let w = p.size(s, f, None).0 + 20.;
                    p.rect(b.at(x, 0., w, b.h), CHAT_USER_RGB, 255, 6.);
                    p.text(b.at(x + 10., 6., w - 20., b.h - 12.), s, f, false);
                    x += w + GAP;
                }
            }
        }
    }
}

fn row_height(p: &mut impl Painter, row: &[&str; CHAT_TABLE_COLUMNS], width: f32) -> f32 {
    row.iter()
        .map(|s| {
            p.size(s, Font::regular(CHAT_CODE_TEXT, WHITE), Some(width))
                .1
        })
        .fold(0., f32::max)
        + 6.
}

fn code_layout(p: &mut impl Painter, area: Option<Box2>, width: f32, code: &str) -> f32 {
    let font = Font::new(CHAT_CODE_TEXT, Face::Mono, WHITE);
    let (advance, line) = p.size("M", font, None);
    let cols = ((width - 16.) / advance.max(1.)).floor().max(1.) as usize;
    let spans = area.map(|_| chat_code_spans(code)).unwrap_or_default();
    let (mut row, mut offset) = (0, 0);
    for source in code.split('\n') {
        let length = source.len();
        for start in (0..length.max(1)).step_by(cols) {
            let end = (start + cols).min(length);
            if let Some(b) = area {
                let mut col = start;
                while col < end {
                    let span = spans.iter().find(|(r, _)| r.end > offset + col);
                    let (until, ink) = match span {
                        Some((r, kind)) if r.start <= offset + col => {
                            ((r.end - offset).min(end), kind.rgb())
                        }
                        Some((r, _)) => ((r.start - offset).min(end), WHITE),
                        None => (end, WHITE),
                    };
                    p.text(
                        b.at(
                            8. + (col - start) as f32 * advance,
                            8. + row as f32 * line,
                            (until - col) as f32 * advance + 1.,
                            line,
                        ),
                        &source[col..until],
                        Font { color: ink, ..font },
                        false,
                    );
                    col = until;
                }
            }
            row += 1;
        }
        offset += length + 1;
    }
    16. + row as f32 * line
}
