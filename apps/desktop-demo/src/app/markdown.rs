use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cranpose_foundation::{
    lazy::rememberLazyListState, text::TextFieldState, SemanticsConfiguration,
};
use cranpose_services::{local_http_client, local_uri_handler, HttpClientRef};
use cranpose_ui::{
    composable,
    text::{
        AnnotatedString, FontFamily, FontStyle, FontWeight, LinkAnnotation, ParagraphStyle,
        PlatformParagraphStyle, SpanStyle, TextDecoration, TextShaping, TextUnit,
    },
    Alignment, Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec, ContentScale,
    CornerRadii, Image, ImageBitmap, LazyColumn, LazyColumnSpec, LinearArrangement, LinkedText,
    Modifier, Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{
    highlight::{language_from_fence, Language},
    highlight_theme::append_highlighted,
    lazy_scrollbar::{LazyListWithScrollbar, LazyScrollbarStyle},
    net_image::decode_bitmap,
    url_resolve::resolve_url,
};

#[derive(Clone, Debug, PartialEq)]
enum MarkdownBlock {
    Text(Rc<AnnotatedString>),
    Image { url: String, alt: String },
    Rule,
}

#[derive(Clone, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    code: bool,
    heading: Option<HeadingLevel>,
    blockquote_depth: u32,
}

impl InlineStyle {
    fn heading_font_size(level: HeadingLevel) -> f32 {
        match level {
            HeadingLevel::H1 => 28.0,
            HeadingLevel::H2 => 24.0,
            HeadingLevel::H3 => 20.0,
            HeadingLevel::H4 => 18.0,
            HeadingLevel::H5 => 16.0,
            HeadingLevel::H6 => 14.0,
        }
    }

    fn to_span_style(&self) -> SpanStyle {
        let font_weight = if self.bold || self.heading.is_some() {
            Some(FontWeight::BOLD)
        } else {
            None
        };
        let font_style = if self.italic {
            Some(FontStyle::Italic)
        } else {
            None
        };
        let font_family = if self.code {
            Some(FontFamily::Monospace)
        } else {
            None
        };
        let font_size = if let Some(level) = self.heading {
            TextUnit::Sp(Self::heading_font_size(level))
        } else {
            TextUnit::Unspecified
        };
        let background = if self.code {
            Some(Color(0.12, 0.12, 0.16, 0.6))
        } else {
            None
        };
        let color = if self.blockquote_depth > 0 {
            Some(Color(0.55, 0.65, 0.85, 1.0))
        } else {
            None
        };
        SpanStyle {
            font_weight,
            font_style,
            font_family,
            font_size,
            background,
            color,
            ..Default::default()
        }
    }
}

struct BlockBuilder {
    style: InlineStyle,
    builder_raw: Option<cranpose_ui::text::annotated_string::Builder>,
    blocks: Vec<MarkdownBlock>,
    list_item_depth: u32,
    in_code_block: bool,
    pending_code_newlines: String,
    code_text: String,
    code_language: Language,
    pending_image: Option<PendingImage>,
    base_url: String,
}

#[derive(Clone, Default)]
struct PendingImage {
    url: String,
    alt: String,
}

impl BlockBuilder {
    fn new(base_url: &str) -> Self {
        Self {
            style: InlineStyle::default(),
            builder_raw: None,
            blocks: Vec::new(),
            list_item_depth: 0,
            in_code_block: false,
            pending_code_newlines: String::new(),
            code_text: String::new(),
            code_language: Language::Plain,
            pending_image: None,
            base_url: base_url.to_string(),
        }
    }

    fn push_inline_style(&mut self) {
        let style = self.style.to_span_style();
        let b = std::mem::take(&mut self.builder_raw).unwrap_or_else(|| {
            let mut b = AnnotatedString::builder();
            if self.style.blockquote_depth > 0 {
                let prefix = "│ ".repeat(self.style.blockquote_depth as usize);
                b = b
                    .push_style(SpanStyle {
                        color: Some(Color(0.40, 0.55, 0.80, 1.0)),
                        ..Default::default()
                    })
                    .append(&prefix)
                    .pop();
            }
            b
        });
        self.builder_raw = if style == SpanStyle::default() {
            Some(b)
        } else {
            Some(b.push_style(style))
        };
    }

    fn pop_style(&mut self) {
        if let Some(b) = self.builder_raw.take() {
            self.builder_raw = Some(b.pop());
        }
    }

    fn push_span_style(&mut self, style: SpanStyle) {
        let b = self
            .builder_raw
            .take()
            .unwrap_or_else(AnnotatedString::builder);
        self.builder_raw = Some(b.push_style(style));
    }

    fn push_link(&mut self, link: LinkAnnotation) {
        let b = self
            .builder_raw
            .take()
            .unwrap_or_else(AnnotatedString::builder);
        self.builder_raw = Some(b.push_link(link));
    }

    fn append(&mut self, text: &str) {
        let b = self
            .builder_raw
            .take()
            .unwrap_or_else(AnnotatedString::builder);
        self.builder_raw = Some(b.append(text));
    }

    fn append_code_text(&mut self, text: &str) {
        let trimmed = text.trim_end_matches(['\n', '\r']);
        if !trimmed.is_empty() {
            if !self.pending_code_newlines.is_empty() {
                let pending = std::mem::take(&mut self.pending_code_newlines);
                self.code_text.push_str(&pending);
            }
            self.code_text.push_str(trimmed);
        }
        let trailing = &text[trimmed.len()..];
        if !trailing.is_empty() {
            self.pending_code_newlines.push_str(trailing);
        }
    }

    fn finish_code_block(&mut self) {
        self.pending_code_newlines.clear();
        self.in_code_block = false;
        let code = std::mem::take(&mut self.code_text);
        if !code.is_empty() {
            let builder = self
                .builder_raw
                .take()
                .unwrap_or_else(AnnotatedString::builder);
            self.builder_raw = Some(append_highlighted(builder, self.code_language, &code));
        }
        self.code_language = Language::Plain;
    }

    fn flush_block(&mut self) {
        if let Some(b) = self.builder_raw.take() {
            let s = b.to_annotated_string();
            if !s.text.is_empty() {
                self.blocks.push(MarkdownBlock::Text(Rc::new(s)));
            }
        }
    }

    fn push_rule(&mut self) {
        self.flush_block();
        self.blocks.push(MarkdownBlock::Rule);
    }

    fn start_image(&mut self, url: String) {
        self.flush_block();
        self.pending_image = Some(PendingImage {
            url,
            alt: String::new(),
        });
    }

    fn finish_image(&mut self) {
        let Some(pending) = self.pending_image.take() else {
            return;
        };
        if pending.url.is_empty() {
            return;
        }
        self.blocks.push(MarkdownBlock::Image {
            url: pending.url,
            alt: pending.alt,
        });
    }
}

fn start_tag(b: &mut BlockBuilder, tag: Tag) {
    match tag {
        Tag::Heading { level, .. } => {
            b.flush_block();
            b.style.heading = Some(level);
            b.push_inline_style();
        }
        Tag::Paragraph if b.list_item_depth == 0 => {
            b.flush_block();
            b.push_inline_style();
        }
        Tag::BlockQuote(_) => {
            b.flush_block();
            b.style.blockquote_depth += 1;
            b.push_inline_style();
        }
        Tag::CodeBlock(kind) => {
            b.flush_block();
            b.style.code = true;
            b.in_code_block = true;
            b.pending_code_newlines.clear();
            b.code_text.clear();
            b.code_language = match &kind {
                CodeBlockKind::Fenced(tag) => language_from_fence(tag),
                CodeBlockKind::Indented => Language::Plain,
            };
            b.push_inline_style();
        }
        Tag::Item => {
            b.flush_block();
            b.list_item_depth += 1;
            b.push_span_style(SpanStyle {
                color: Some(Color(0.55, 0.65, 0.85, 1.0)),
                ..Default::default()
            });
            b.append("• ");
            b.pop_style();
            b.push_inline_style();
        }
        Tag::Emphasis => {
            b.style.italic = true;
            b.push_inline_style();
        }
        Tag::Strong => {
            b.style.bold = true;
            b.push_inline_style();
        }
        Tag::Link { dest_url, .. } => {
            b.push_link(LinkAnnotation::Url(resolve_url(&b.base_url, &dest_url)));
            b.push_span_style(SpanStyle {
                color: Some(Color(0.35, 0.65, 0.95, 1.0)),
                text_decoration: Some(TextDecoration::UNDERLINE),
                ..Default::default()
            });
        }
        Tag::Image { dest_url, .. } => {
            b.start_image(resolve_url(&b.base_url, &dest_url));
        }
        _ => {}
    }
}

fn end_tag(b: &mut BlockBuilder, tag: TagEnd) {
    match tag {
        TagEnd::Heading(_) => {
            b.pop_style();
            b.style.heading = None;
            b.flush_block();
        }
        TagEnd::Paragraph if b.list_item_depth == 0 => {
            b.pop_style();
            b.flush_block();
        }
        TagEnd::BlockQuote(_) => {
            b.pop_style();
            b.style.blockquote_depth = b.style.blockquote_depth.saturating_sub(1);
            b.flush_block();
        }
        TagEnd::CodeBlock => {
            b.finish_code_block();
            b.pop_style();
            b.style.code = false;
            b.flush_block();
        }
        TagEnd::Item => {
            b.pop_style();
            b.flush_block();
            b.list_item_depth = b.list_item_depth.saturating_sub(1);
        }
        TagEnd::Emphasis => {
            b.pop_style();
            b.style.italic = false;
        }
        TagEnd::Strong => {
            b.pop_style();
            b.style.bold = false;
        }
        TagEnd::Link => {
            b.pop_style();
            b.pop_style();
        }
        TagEnd::Image => {
            b.finish_image();
        }
        _ => {}
    }
}

fn markdown_to_blocks(markdown: &str, base_url: &str) -> Vec<MarkdownBlock> {
    let options = Options::empty();
    let parser = Parser::new_ext(markdown, options);

    let mut b = BlockBuilder::new(base_url);

    for event in parser {
        match event {
            Event::Start(tag) => start_tag(&mut b, tag),
            Event::End(tag) => end_tag(&mut b, tag),
            Event::Code(text) => {
                b.push_span_style(SpanStyle {
                    font_family: Some(FontFamily::Monospace),
                    background: Some(Color(0.12, 0.12, 0.16, 0.6)),
                    ..Default::default()
                });
                b.append(&text);
                b.pop_style();
            }
            Event::Text(text) => {
                if let Some(pending) = b.pending_image.as_mut() {
                    pending.alt.push_str(&text);
                } else if b.in_code_block {
                    b.append_code_text(&text);
                } else {
                    b.append(&text);
                }
            }
            Event::SoftBreak => b.append(" "),
            Event::HardBreak => b.append("\n"),
            Event::Rule => b.push_rule(),

            _ => {}
        }
    }

    b.flush_block();
    b.blocks
}

const MAX_MARKDOWN_BLOCK_BYTES: usize = 1200;

fn split_large_markdown_blocks(blocks: Vec<MarkdownBlock>) -> Vec<MarkdownBlock> {
    let mut normalized = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            MarkdownBlock::Text(annotated) if annotated.text.len() > MAX_MARKDOWN_BLOCK_BYTES => {
                split_large_text_block(&annotated, &mut normalized);
            }
            other => normalized.push(other),
        }
    }
    normalized
}

fn split_large_text_block(annotated: &AnnotatedString, out: &mut Vec<MarkdownBlock>) {
    let text = annotated.text.as_str();
    let mut start = 0usize;

    while start < text.len() {
        let mut end = (start + MAX_MARKDOWN_BLOCK_BYTES).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = text[start..]
                .char_indices()
                .nth(1)
                .map_or(text.len(), |(offset, _)| start + offset);
        } else if end < text.len() {
            let split_window = &text[start..end];
            if let Some(rel_newline) = split_window.rfind('\n') {
                let candidate = start + rel_newline + 1;
                let min_chunk = start + (MAX_MARKDOWN_BLOCK_BYTES / 3);
                if candidate >= min_chunk {
                    end = candidate;
                }
            }
        }

        out.push(MarkdownBlock::Text(Rc::new(
            annotated.subsequence(start..end),
        )));
        start = end;
    }
}

#[derive(Clone, Debug, PartialEq)]
enum FetchState {
    Idle,
    Loading,
    Done(Rc<[MarkdownBlock]>),
    Error(String),
}

async fn fetch_markdown(client: &HttpClientRef, url: &str) -> Result<String, String> {
    client
        .get_text(url)
        .await
        .map_err(|e| format!("Request failed: {e}"))
}

const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/samoylenkodmitry/s-a--m.github.io/refs/heads/master/_leetcode_source/2023-07-14-leetcode_daily.md";

#[composable]
pub fn markdown_viewer_tab() {
    let url_state = cranpose_core::remember(|| TextFieldState::new(DEFAULT_URL)).with(|s| *s);
    let fetch_state = cranpose_core::rememberMutableStateOf(|| FetchState::Idle);
    let request_counter = cranpose_core::rememberMutableStateOf(|| 0u64);
    let http_client = local_http_client().current();

    cranpose_core::LaunchedEffect(request_counter.get(), move |scope| {
        let tick = request_counter.get();
        if tick == 0 {
            return;
        }

        let url = url_state.text();
        let url = url.trim().to_string();
        let document_url = url.clone();
        fetch_state.set(FetchState::Loading);

        let client = http_client.clone();
        scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return Err("request cancelled".to_string());
                }
                if url.is_empty() {
                    return Err("URL is empty".to_string());
                }
                fetch_markdown(&client, &url).await
            },
            move |result| match result {
                Ok(text) => {
                    let blocks: Rc<[MarkdownBlock]> =
                        split_large_markdown_blocks(markdown_to_blocks(&text, &document_url))
                            .into();
                    fetch_state.set(FetchState::Done(blocks));
                }
                Err(err) => fetch_state.set(FetchState::Error(err)),
            },
        );
    });

    Column(
        Modifier::empty()
            .padding(16.0)
            .background(Color(0.06, 0.08, 0.14, 1.0))
            .rounded_corners(20.0)
            .padding(16.0)
            .fill_max_size(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        {
            move || {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        move || {
                            cranpose_ui::BasicTextField(
                                url_state,
                                Modifier::empty()
                                    .weight(1.0)
                                    .padding(10.0)
                                    .background(Color(0.12, 0.14, 0.22, 1.0))
                                    .rounded_corners(10.0),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(Color(0.82, 0.86, 0.95, 1.0)),
                                        font_size: TextUnit::Sp(12.0),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );
                            Button(
                                Modifier::empty()
                                    .rounded_corners(10.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::linear_gradient(vec![
                                                Color(0.22, 0.52, 0.92, 1.0),
                                                Color(0.14, 0.38, 0.78, 1.0),
                                            ]),
                                            CornerRadii::uniform(10.0),
                                        );
                                    })
                                    .padding(10.0),
                                ButtonSpec::default(),
                                move || request_counter.update(|v| *v = v.wrapping_add(1)),
                                || {
                                    Text(
                                        "Fetch",
                                        Modifier::empty().padding(4.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 1.0, 1.0, 1.0)),
                                                font_weight: Some(FontWeight::BOLD),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                },
                            );
                        }
                    },
                );

                match fetch_state.get() {
                    FetchState::Idle => {
                        Text(
                            "Enter a URL pointing to a raw Markdown file and press Fetch.",
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.10, 0.14, 0.24, 0.8))
                                .rounded_corners(12.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.65, 0.70, 0.85, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                    FetchState::Loading => {
                        Text(
                            "Fetching…",
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.14, 0.20, 0.38, 0.9))
                                .rounded_corners(12.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.75, 0.82, 1.0, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                    FetchState::Error(msg) => {
                        Text(
                            format!("Error: {msg}"),
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.40, 0.12, 0.12, 0.9))
                                .rounded_corners(12.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 0.65, 0.65, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                    FetchState::Done(blocks) => {
                        render_markdown_blocks(blocks);
                    }
                }
            }
        },
    );
}

pub const MARKDOWN_SCROLL_STABILITY_TARGET_TEXT: &str =
    "Stability paragraph 032 keeps glyphs, background cards, and links moving as one rigid surface.";

#[composable]
pub fn MarkdownScrollStabilityFixtureTab() {
    let blocks = cranpose_core::remember(|| {
        let markdown = scroll_stability_fixture_markdown();
        Rc::<[MarkdownBlock]>::from(split_large_markdown_blocks(markdown_to_blocks(
            &markdown, "",
        )))
    })
    .with(|blocks| blocks.clone());

    Column(
        Modifier::empty()
            .padding(16.0)
            .background(Color(0.06, 0.08, 0.14, 1.0))
            .rounded_corners(20.0)
            .padding(16.0)
            .fill_max_size(),
        ColumnSpec::default(),
        move || {
            render_markdown_blocks(blocks.clone());
        },
    );
}

#[composable]
pub fn MarkdownScrollStressFixtureTab() {
    let list_state = rememberLazyListState();
    MarkdownScrollStressFixtureTabWithState(list_state);
}

#[composable]
pub fn MarkdownScrollStressFixtureTabWithState(
    list_state: cranpose_foundation::lazy::LazyListState,
) {
    let blocks = cranpose_core::remember(|| {
        let markdown = markdown_scroll_stress_fixture();
        Rc::<[MarkdownBlock]>::from(split_large_markdown_blocks(markdown_to_blocks(
            &markdown, "",
        )))
    })
    .with(|blocks| blocks.clone());

    Column(
        Modifier::empty()
            .padding(16.0)
            .background(Color(0.06, 0.08, 0.14, 1.0))
            .rounded_corners(20.0)
            .padding(16.0)
            .fill_max_size(),
        ColumnSpec::default(),
        move || {
            render_markdown_blocks_with_state(blocks.clone(), list_state);
        },
    );
}

fn scroll_stability_fixture_markdown() -> String {
    let mut markdown = String::from("# Markdown Scroll Stability Fixture\n\n");
    for index in 1..=96 {
        if index == 32 {
            markdown.push_str(MARKDOWN_SCROLL_STABILITY_TARGET_TEXT);
        } else if index % 9 == 0 {
            markdown.push_str(&format!(
                "Fixture paragraph {index:03} includes [a deterministic link](https://example.com/{index:03}) so linked text follows the same scroll anchor."
            ));
        } else if index % 5 == 0 {
            markdown.push_str(&format!(
                "Fixture paragraph {index:03} mixes **bold text** with _italic text_ to keep styled spans in the stability contract."
            ));
        } else {
            markdown.push_str(&format!(
                "Fixture paragraph {index:03} is plain markdown text with enough width to exercise multi-line text layout during exact scrolling."
            ));
        }
        markdown.push_str("\n\n");
    }
    markdown
}

pub fn markdown_scroll_stress_fixture() -> String {
    let mut markdown = String::from("# Markdown Scroll Stress Fixture\n\n");
    for index in 1..=420 {
        if index % 17 == 0 {
            markdown.push_str(&format!(
                "### Section {index:03}\n\nThis section heading is followed by a longer paragraph with [linked source material](https://example.com/{index:03}) and enough text to wrap across multiple lines inside the Markdown viewport."
            ));
        } else if index % 11 == 0 {
            markdown.push_str(&format!(
                "> Quote block {index:03} keeps a distinct visual band while scrolling, with **bold emphasis**, _italic emphasis_, and inline `code` in the same block."
            ));
        } else if index % 7 == 0 {
            markdown.push_str(&format!(
                "- Line {index:03} combines list indentation, a deterministic URL https://example.com/items/{index:03}, and enough trailing prose to force text measurement cache reuse during fast scroll."
            ));
        } else {
            markdown.push_str(&format!(
                "Paragraph {index:03} is representative fetched Markdown content with plain text, **strong spans**, _emphasis spans_, inline `tokens`, and wrapping sentences that should scroll at the production frame budget."
            ));
        }
        markdown.push_str("\n\n");
    }
    markdown
}

const MARKDOWN_SCROLLBAR_RAIL_WIDTH: f32 = 16.0;
const MARKDOWN_SCROLLBAR_THUMB_WIDTH: f32 = 8.0;
const MARKDOWN_SCROLLBAR_MIN_THUMB_HEIGHT: f32 = 32.0;

#[composable]
fn MarkdownBlocksList(
    list_state: cranpose_foundation::lazy::LazyListState,
    blocks: Rc<[MarkdownBlock]>,
) {
    let mut spec = LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0));
    spec.beyond_bounds_item_count = 0;
    LazyColumn(
        Modifier::empty()
            .semantics(|config: &mut SemanticsConfiguration| {
                config.content_description = Some("MarkdownListViewport".to_string());
            })
            .fill_max_size(),
        list_state,
        spec,
        move |scope| {
            use cranpose_foundation::lazy::LazyListScopeExt;
            scope.items_indexed_rc(blocks, |_index, block| match block {
                MarkdownBlock::Text(annotated) => render_text_block(annotated.clone()),
                MarkdownBlock::Image { url, alt } => {
                    MarkdownImage(url.clone(), alt.clone());
                }
                MarkdownBlock::Rule => render_rule(),
            });
        },
    );
}

fn markdown_scrollbar_style() -> LazyScrollbarStyle {
    LazyScrollbarStyle {
        rail_width: MARKDOWN_SCROLLBAR_RAIL_WIDTH,
        thumb_width: MARKDOWN_SCROLLBAR_THUMB_WIDTH,
        min_thumb_height: MARKDOWN_SCROLLBAR_MIN_THUMB_HEIGHT,
        rail_color: Color(0.12, 0.15, 0.24, 1.0),
        thumb_color: Color(0.55, 0.68, 1.0, 0.90),
    }
}

#[composable]
fn render_markdown_blocks(blocks: Rc<[MarkdownBlock]>) {
    let list_state = rememberLazyListState();
    render_markdown_blocks_with_state(blocks, list_state);
}

#[composable]
fn render_markdown_blocks_with_state(
    blocks: Rc<[MarkdownBlock]>,
    list_state: cranpose_foundation::lazy::LazyListState,
) {
    LazyListWithScrollbar(
        Modifier::empty().fill_max_size(),
        list_state,
        "MarkdownScrollbarRail",
        markdown_scrollbar_style(),
        move || {
            MarkdownBlocksList(list_state, blocks.clone());
        },
    );
}

#[composable]
fn render_text_block(annotated: Rc<AnnotatedString>) {
    let text_style = TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.88, 0.90, 0.96, 1.0)),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        paragraph_style: ParagraphStyle {
            platform_style: Some(PlatformParagraphStyle {
                include_font_padding: None,
                shaping: Some(TextShaping::Basic),
            }),
            ..Default::default()
        },
    };

    if !annotated.link_annotations.is_empty() {
        let uri_handler = local_uri_handler().current();
        LinkedText(
            (*annotated).clone(),
            Modifier::empty().fill_max_width().padding(2.0),
            text_style,
            move |url| {
                if let Err(err) = uri_handler.open_uri(url) {
                    log::error!("Failed to open URL {url}: {err:#}");
                }
            },
        );
    } else {
        Text(
            annotated,
            Modifier::empty().fill_max_width().padding(2.0),
            text_style,
        );
    }
}

const MARKDOWN_IMAGE_HEIGHT: f32 = 220.0;
const MARKDOWN_IMAGE_CACHE_CAPACITY: usize = 24;

#[derive(Default)]
struct ImageCache {
    entries: HashMap<String, ImageBitmap>,
    order: Vec<String>,
}

impl ImageCache {
    fn get(&self, url: &str) -> Option<ImageBitmap> {
        self.entries.get(url).cloned()
    }

    fn insert(&mut self, url: String, bitmap: ImageBitmap) {
        if self.entries.contains_key(&url) {
            return;
        }
        while self.order.len() >= MARKDOWN_IMAGE_CACHE_CAPACITY {
            let evicted = self.order.remove(0);
            self.entries.remove(&evicted);
        }
        self.order.push(url.clone());
        self.entries.insert(url, bitmap);
    }
}

thread_local! {
    static MARKDOWN_IMAGE_CACHE: RefCell<ImageCache> = RefCell::new(ImageCache::default());
}

fn cached_image(url: &str) -> Option<ImageBitmap> {
    MARKDOWN_IMAGE_CACHE.with(|cache| cache.borrow().get(url))
}

fn cache_image(url: String, bitmap: ImageBitmap) {
    MARKDOWN_IMAGE_CACHE.with(|cache| cache.borrow_mut().insert(url, bitmap));
}

async fn fetch_image(client: &HttpClientRef, url: &str) -> Result<ImageBitmap, String> {
    let bytes = client
        .get_bytes(url)
        .await
        .map_err(|err| format!("failed to download image: {err}"))?;
    decode_bitmap(&bytes).map_err(|err| format!("failed to decode image: {err}"))
}

#[derive(Clone, Debug, PartialEq)]
enum ImageState {
    Loading,
    Ready(ImageBitmap),
    Error(String),
}

/// A markdown image, fetched the first time it is composed.
///
/// The blocks list is a `LazyColumn` with no beyond-bounds items, so this
/// composable only runs once its block scrolls into view — that, rather than
/// any explicit visibility test, is what makes the fetch lazy. The slot keeps
/// a fixed height whether or not the bitmap has arrived, so a late image
/// cannot shift the rows the reader is looking at.
#[composable]
fn MarkdownImage(url: String, alt: String) {
    let cached = cached_image(&url);
    let state = cranpose_core::rememberMutableStateOf(|| match cached.clone() {
        Some(bitmap) => ImageState::Ready(bitmap),
        None => ImageState::Loading,
    });
    let http_client = local_http_client().current();

    let effect_url = url.clone();
    cranpose_core::LaunchedEffect(url, move |scope| {
        if let Some(bitmap) = cached_image(&effect_url) {
            state.set(ImageState::Ready(bitmap));
            return;
        }
        state.set(ImageState::Loading);
        let client = http_client.clone();
        let fetch_url = effect_url.clone();
        let store_url = effect_url.clone();
        scope.launch_background(
            move |_token| async move { fetch_image(&client, &fetch_url).await },
            move |result| match result {
                Ok(bitmap) => {
                    cache_image(store_url.clone(), bitmap.clone());
                    state.set(ImageState::Ready(bitmap));
                }
                Err(err) => state.set(ImageState::Error(err)),
            },
        );
    });

    let description = if alt.is_empty() {
        "Markdown image".to_string()
    } else {
        alt.clone()
    };
    match state.get() {
        ImageState::Ready(bitmap) => {
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(MARKDOWN_IMAGE_HEIGHT)
                    .background(Color(0.10, 0.12, 0.17, 1.0))
                    .rounded_corners(8.0),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    Image(
                        bitmap.clone(),
                        Some(description.clone()),
                        Modifier::empty().fill_max_size(),
                        Alignment::CENTER,
                        ContentScale::Fit,
                        1.0,
                        None,
                    );
                },
            );
        }
        ImageState::Loading => {
            Text(
                placeholder_label(&alt),
                Modifier::empty().padding(8.0),
                placeholder_text_style(Color(0.70, 0.75, 0.85, 1.0)),
            );
        }
        ImageState::Error(err) => {
            Text(
                err,
                Modifier::empty().padding(8.0),
                placeholder_text_style(Color(0.92, 0.72, 0.72, 1.0)),
            );
        }
    }
}

fn placeholder_label(alt: &str) -> String {
    if alt.is_empty() {
        "Loading image…".to_string()
    } else {
        format!("Loading {alt}…")
    }
}

fn placeholder_text_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(13.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[composable]
fn render_rule() {
    Spacer(Size {
        width: 0.0,
        height: 4.0,
    });
    cranpose_ui::Box(
        Modifier::empty()
            .fill_max_width()
            .draw_behind(|scope| {
                let size = scope.size();
                scope.draw_rect_at(
                    cranpose_ui::Rect {
                        x: 0.0,
                        y: 0.0,
                        width: size.width,
                        height: 2.0,
                    },
                    Brush::solid(Color(0.35, 0.40, 0.55, 0.5)),
                );
            })
            .size(cranpose_ui::Size {
                width: f32::INFINITY,
                height: 2.0,
            }),
        cranpose_ui::BoxSpec::default(),
        || {},
    );
    Spacer(Size {
        width: 0.0,
        height: 4.0,
    });
}

#[cfg(test)]
#[path = "tests/markdown_tests.rs"]
mod tests;
