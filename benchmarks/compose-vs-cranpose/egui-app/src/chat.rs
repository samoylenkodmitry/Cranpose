//! The chat beside the patchwork in egui (`../README.md`): an agent's
//! answers arrive from a data thread as tokens and the screen shows them as
//! they come.
//!
//! egui builds the chat again every frame from its widgets: labels, a grid,
//! buttons and images in a scroll area that keeps to its end. The scroll
//! area holds the blocks of the last two turns, more than the chat shows:
//! an immediate list lays out every block it holds each frame, so the app
//! holds no more than can show. egui keeps the layout of a text it showed
//! the frame before, so only the block that grows is laid out anew. It has
//! no gradient for text: the status line colors each glyph by its place.

use eframe::egui::{
    self, Align, Button, Color32, FontFamily, FontId, Frame, Grid, Image, ImageSource, Label,
    Layout, Margin, Rect, RichText, ScrollArea, TextFormat, TextureHandle, TextureOptions, Ui,
    UiBuilder, Vec2, pos2, scroll_area::ScrollBarVisibility, text::LayoutJob, vec2,
};
use perf_data::*;

use super::{
    bold,
    wall::{INK, rgb},
};

const BACKGROUND: Color32 = rgb(CHAT_BACKGROUND_RGB);
const PANEL: Color32 = rgb(CHAT_PANEL_RGB);
const BUBBLE: Color32 = rgb(CHAT_USER_RGB);
// The space around the list and between its blocks, in points.
const GAP: f32 = 8.0;
// The turns whose blocks the scroll area holds.
const TURNS_HELD: usize = 2;

/// The chat's state: the data thread, the last token it sent and the
/// animated picture's frames as textures.
pub struct Chat {
    feed: Option<ChatFeed>,
    token: Option<ChatToken>,
    gif: Vec<TextureHandle>,
}

fn format(size: f32, family: FontFamily, color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::new(size, family),
        color,
        line_height: Some(size * 1.4),
        ..Default::default()
    }
}

// Every frame of the GIF as a texture; none when it does not decode.
fn gif_textures(ctx: &egui::Context) -> Vec<TextureHandle> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut textures = Vec::new();
    match options.read_info(CHAT_GIF) {
        Ok(mut decoder) => {
            let size = [usize::from(decoder.width()), usize::from(decoder.height())];
            while let Ok(Some(frame)) = decoder.read_next_frame() {
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.buffer);
                let name = format!("chat-picture-{}", textures.len());
                textures.push(ctx.load_texture(name, image, TextureOptions::LINEAR));
            }
        }
        Err(error) => log::warn!("chat picture: {error}"),
    }
    textures
}

impl Chat {
    pub fn new(ctx: &egui::Context) -> Self {
        egui_extras::install_image_loaders(ctx);
        Self {
            feed: ChatFeed::start()
                .map_err(|error| log::warn!("chat data thread: {error}"))
                .ok(),
            token: None,
            gif: gif_textures(ctx),
        }
    }

    /// Takes the frame's tokens and shows the chat in `rect`.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, frame: u32) {
        if let Some(feed) = &self.feed {
            for _ in 0..CHAT_TOKENS_PER_FRAME {
                match feed.take() {
                    Some(token) => self.token = Some(token),
                    None => break,
                }
            }
        }
        ui.painter().rect_filled(rect, 0.0, BACKGROUND);
        let status = Rect::from_min_max(
            pos2(rect.left(), rect.bottom() - CHAT_STATUS_HEIGHT),
            rect.max,
        );
        let list = Rect::from_min_max(rect.min, status.right_top()).shrink(GAP);
        let Some(token) = self.token else {
            return;
        };
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ui.set_clip_rect(list);
            ui.spacing_mut().item_spacing = Vec2::splat(GAP);
            ui.style_mut().interaction.selectable_labels = false;
            ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink(false)
                .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    let first = (token.block + 1).saturating_sub(TURNS_HELD * CHAT_TURN.len());
                    for block in first..=token.block {
                        let shown = (block == token.block).then_some(token.shown);
                        self.block(ui, block, shown, frame);
                    }
                });
        });
        Self::status(ui, status, token, frame);
    }

    // Block `block`: whole, or as far as `shown` tokens show the last one.
    fn block(&self, ui: &mut Ui, block: usize, shown: Option<usize>, frame: u32) {
        let turn = ChatToken::turn(block);
        match CHAT_TURN[block % CHAT_TURN.len()] {
            ChatBlock::User(text) => {
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    Frame::new()
                        .fill(BUBBLE)
                        .corner_radius(10.0)
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| Self::text(ui, text.to_owned()));
                });
            }
            ChatBlock::Paragraph(text) => {
                let chars = shown.map_or(text.len(), |shown| chat_shown_chars(text, shown));
                Self::text(ui, format!("{} {}", chat_turn_label(turn), &text[..chars]));
            }
            ChatBlock::Code(code) => {
                let chars = shown.map_or(code.len(), |shown| chat_shown_chars(code, shown));
                Frame::new()
                    .fill(PANEL)
                    .corner_radius(6.0)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(Label::new(Self::code(turn, &code[..chars])).wrap());
                    });
            }
            ChatBlock::Table(rows) => {
                let count = shown.map_or(rows.len(), |shown| shown.min(rows.len()));
                let column = (ui.available_width() - 3.0 * GAP) / CHAT_TABLE_COLUMNS as f32;
                Grid::new(("chat-table", block))
                    .num_columns(CHAT_TABLE_COLUMNS)
                    .striped(true)
                    .min_col_width(column)
                    .max_col_width(column)
                    .spacing(vec2(GAP, 3.0))
                    .show(ui, |ui| {
                        for (index, row) in rows[..count].iter().enumerate() {
                            let family = if index == 0 {
                                bold()
                            } else {
                                FontFamily::Proportional
                            };
                            for cell in row {
                                let text = RichText::new(*cell)
                                    .font(FontId::new(CHAT_CODE_TEXT, family.clone()))
                                    .color(INK);
                                ui.label(text);
                            }
                            ui.end_row();
                        }
                    });
            }
            ChatBlock::Gif => {
                if !self.gif.is_empty() {
                    let picture = &self.gif[chat_gif_frame(frame, self.gif.len())];
                    ui.add(
                        Image::from_texture(picture).fit_to_exact_size(Vec2::splat(CHAT_GIF_SIZE)),
                    );
                }
            }
            ChatBlock::Buttons(labels) => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().button_padding = vec2(10.0, 6.0);
                    for label in labels {
                        let text = RichText::new(*label)
                            .font(FontId::new(CHAT_TEXT, bold()))
                            .color(INK);
                        let _ = ui.add(Button::new(text).fill(BUBBLE).corner_radius(6.0));
                    }
                });
            }
        }
    }

    // A paragraph of the chat's text, wrapped at the width left.
    fn text(ui: &mut Ui, text: String) {
        let job = LayoutJob::single_section(text, format(CHAT_TEXT, FontFamily::Proportional, INK));
        ui.add(Label::new(job).wrap());
    }

    // `code` under a comment that names its turn, each stretch in its color.
    fn code(turn: usize, code: &str) -> LayoutJob {
        let mono = |color| format(CHAT_CODE_TEXT, FontFamily::Monospace, color);
        let mut job = LayoutJob::default();
        let head = format!("// {}\n", chat_turn_label(turn));
        job.append(&head, 0.0, mono(rgb(CodeKind::Comment.rgb())));
        let mut at = 0;
        for (span, kind) in chat_code_spans(code) {
            job.append(&code[at..span.start], 0.0, mono(INK));
            job.append(&code[span.clone()], 0.0, mono(rgb(kind.rgb())));
            at = span.end;
        }
        job.append(&code[at..], 0.0, mono(INK));
        job
    }

    // The status line: the spinner turned to the frame's angle, and what the
    // agent does with each glyph in the gradient's color at its place.
    fn status(ui: &Ui, rect: Rect, token: ChatToken, frame: u32) {
        ui.painter().rect_filled(rect, 0.0, PANEL);
        let spinner = Rect::from_center_size(
            pos2(rect.left() + GAP + CHAT_SPINNER_SIZE / 2.0, rect.center().y),
            Vec2::splat(CHAT_SPINNER_SIZE),
        );
        Image::new(ImageSource::Bytes {
            uri: "bytes://chat-spinner.svg".into(),
            bytes: CHAT_SVG.as_bytes().into(),
        })
        .rotate(
            (frame as f32 * CHAT_SPIN_DEGREES).to_radians(),
            Vec2::splat(0.5),
        )
        .paint_at(ui, spinner);

        let [dim, bright] = CHAT_SHIMMER_RGB;
        let text = chat_status(token);
        let plain = format(CHAT_TEXT, FontFamily::Proportional, INK);
        let places = ui.ctx().fonts_mut(|fonts| {
            fonts.layout_job(LayoutJob::single_section(text.clone(), plain.clone()))
        });
        let mut job = LayoutJob::default();
        let glyphs = places.rows.iter().flat_map(|row| &row.row.glyphs);
        for (glyph, (start, character)) in glyphs.zip(text.char_indices()) {
            let weight = chat_shimmer(glyph.pos.x, frame);
            let channel = |index: usize| {
                (f32::from(dim[index])
                    + (f32::from(bright[index]) - f32::from(dim[index])) * weight)
                    as u8
            };
            let color = Color32::from_rgb(channel(0), channel(1), channel(2));
            job.append(
                &text[start..start + character.len_utf8()],
                0.0,
                TextFormat {
                    color,
                    ..plain.clone()
                },
            );
        }
        let colored = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
        let at = pos2(
            spinner.right() + 6.0,
            rect.center().y - colored.size().y / 2.0,
        );
        ui.painter().galley(at, colored, INK);
    }
}
