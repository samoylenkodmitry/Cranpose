use super::lazy_scrollbar::{LazyListWithScrollbar, LazyScrollbarStyle};
use cranpose_foundation::lazy::remember_lazy_list_state;
use cranpose_foundation::text::TextFieldState;
use cranpose_foundation::SemanticsConfiguration;
use cranpose_services::{local_http_client, local_uri_handler, HttpClientRef};
use cranpose_ui::{
    composable,
    text::{
        AnnotatedString, FontFamily, FontStyle, FontWeight, LinkAnnotation, ParagraphStyle,
        PlatformParagraphStyle, SpanStyle, TextDecoration, TextShaping, TextUnit,
    },
    Brush, Button, ButtonSpec, Color, Column, ColumnSpec, CornerRadii, LazyColumn, LazyColumnSpec,
    LinearArrangement, LinkedText, Modifier, Row, RowSpec, Size, Spacer, Text, TextStyle,
    VerticalAlignment,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Markdown → AnnotatedString block list
// ---------------------------------------------------------------------------

/// One rendered "block" of markdown content.
#[derive(Clone, Debug, PartialEq)]
enum MarkdownBlock {
    /// A styled paragraph of inline text (may contain link annotations for clickable links).
    Text(Rc<AnnotatedString>),
    /// A horizontal divider (---).
    Rule,
}

/// Inline style stack tracking what styles are currently open.
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

// ---------------------------------------------------------------------------
// Thin builder wrapper: keeps style depth so we don't expose it to pulldown-cmark event handler.
// ---------------------------------------------------------------------------
struct BlockBuilder {
    style: InlineStyle,
    builder_raw: Option<cranpose_ui::text::annotated_string::Builder>,
    blocks: Vec<MarkdownBlock>,
    list_item_depth: u32,
    in_code_block: bool,
    pending_code_newlines: String,
}

impl BlockBuilder {
    fn new() -> Self {
        Self {
            style: InlineStyle::default(),
            builder_raw: None,
            blocks: Vec::new(),
            list_item_depth: 0,
            in_code_block: false,
            pending_code_newlines: String::new(),
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
                self.append(&pending);
            }
            self.append(trimmed);
        }
        let trailing = &text[trimmed.len()..];
        if !trailing.is_empty() {
            self.pending_code_newlines.push_str(trailing);
        }
    }

    fn finish_code_block(&mut self) {
        self.pending_code_newlines.clear();
        self.in_code_block = false;
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
}

/// Convert raw markdown text into a list of styled blocks.
fn markdown_to_blocks(markdown: &str) -> Vec<MarkdownBlock> {
    let options = Options::empty();
    let parser = Parser::new_ext(markdown, options);

    let mut b = BlockBuilder::new();

    for event in parser {
        match event {
            // ---- Block start ----
            Event::Start(Tag::Heading { level, .. }) => {
                b.flush_block();
                b.style.heading = Some(level);
                b.push_inline_style();
            }
            Event::Start(Tag::Paragraph) if b.list_item_depth == 0 => {
                b.flush_block();
                b.push_inline_style();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                b.flush_block();
                b.style.blockquote_depth += 1;
                b.push_inline_style();
            }
            Event::Start(Tag::CodeBlock(_)) => {
                b.flush_block();
                b.style.code = true;
                b.in_code_block = true;
                b.pending_code_newlines.clear();
                b.push_inline_style();
            }
            Event::Start(Tag::Item) => {
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
            Event::Start(Tag::Emphasis) => {
                b.style.italic = true;
                b.push_inline_style();
            }
            Event::Start(Tag::Strong) => {
                b.style.bold = true;
                b.push_inline_style();
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                // Store the URL as a LinkAnnotation::Url — auto-handled by LinkedText.
                b.push_link(LinkAnnotation::Url(dest_url.to_string()));
                b.push_span_style(SpanStyle {
                    color: Some(Color(0.35, 0.65, 0.95, 1.0)),
                    text_decoration: Some(TextDecoration::UNDERLINE),
                    ..Default::default()
                });
            }
            Event::Start(Tag::Image { .. }) => {
                b.push_span_style(SpanStyle {
                    color: Some(Color(0.55, 0.55, 0.55, 1.0)),
                    ..Default::default()
                });
                b.append("[image: ");
            }
            // Inline code
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
                if b.in_code_block {
                    b.append_code_text(&text);
                } else {
                    b.append(&text);
                }
            }
            Event::SoftBreak => b.append(" "),
            Event::HardBreak => b.append("\n"),
            Event::Rule => b.push_rule(),

            // ---- Block end ----
            Event::End(TagEnd::Heading(_)) => {
                b.pop_style();
                b.style.heading = None;
                b.flush_block();
            }
            Event::End(TagEnd::Paragraph) if b.list_item_depth == 0 => {
                b.pop_style();
                b.flush_block();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                b.pop_style();
                b.style.blockquote_depth = b.style.blockquote_depth.saturating_sub(1);
                b.flush_block();
            }
            Event::End(TagEnd::CodeBlock) => {
                b.finish_code_block();
                b.pop_style();
                b.style.code = false;
                b.flush_block();
            }
            Event::End(TagEnd::Item) => {
                b.pop_style();
                b.flush_block();
                b.list_item_depth = b.list_item_depth.saturating_sub(1);
            }
            Event::End(TagEnd::Emphasis) => {
                b.pop_style();
                b.style.italic = false;
            }
            Event::End(TagEnd::Strong) => {
                b.pop_style();
                b.style.bold = false;
            }
            Event::End(TagEnd::Link) => {
                // Pop span style then link annotation (LIFO)
                b.pop_style();
                b.pop_style();
            }
            Event::End(TagEnd::Image) => {
                b.append("]");
                b.pop_style();
            }
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
                .map(|(offset, _)| start + offset)
                .unwrap_or(text.len());
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

// ---------------------------------------------------------------------------
// Fetch state
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Composable
// ---------------------------------------------------------------------------

#[allow(non_snake_case)]
#[composable]
pub fn markdown_viewer_tab() {
    let url_state = cranpose_core::remember(|| TextFieldState::new(DEFAULT_URL)).with(|s| *s);
    let fetch_state = cranpose_core::useState(|| FetchState::Idle);
    let request_counter = cranpose_core::useState(|| 0u64);
    let http_client = local_http_client().current();

    cranpose_core::LaunchedEffect!(request_counter.get(), move |scope| {
        let tick = request_counter.get();
        if tick == 0 {
            return;
        }

        let url = url_state.text();
        let url = url.trim().to_string();
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
                // Only the raw text crosses the thread boundary — AnnotatedString
                // (which may contain Rc inside LinkAnnotation) is built on the main thread.
                fetch_markdown(&client, &url).await
            },
            move |result| match result {
                Ok(text) => {
                    let blocks: Rc<[MarkdownBlock]> =
                        split_large_markdown_blocks(markdown_to_blocks(&text)).into();
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
                // ---- URL input row ----
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

                // ---- Status / content area ----
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

#[allow(non_snake_case)]
#[composable]
pub fn MarkdownScrollStabilityFixtureTab() {
    let blocks = cranpose_core::remember(|| {
        let markdown = scroll_stability_fixture_markdown();
        Rc::<[MarkdownBlock]>::from(split_large_markdown_blocks(markdown_to_blocks(&markdown)))
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

#[allow(non_snake_case)]
#[composable]
pub fn MarkdownScrollStressFixtureTab() {
    let list_state = remember_lazy_list_state();
    MarkdownScrollStressFixtureTabWithState(list_state);
}

#[allow(non_snake_case)]
#[composable]
pub fn MarkdownScrollStressFixtureTabWithState(
    list_state: cranpose_foundation::lazy::LazyListState,
) {
    let blocks = cranpose_core::remember(|| {
        let markdown = markdown_scroll_stress_fixture();
        Rc::<[MarkdownBlock]>::from(split_large_markdown_blocks(markdown_to_blocks(&markdown)))
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

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

const MARKDOWN_SCROLLBAR_RAIL_WIDTH: f32 = 16.0;
const MARKDOWN_SCROLLBAR_THUMB_WIDTH: f32 = 8.0;
const MARKDOWN_SCROLLBAR_MIN_THUMB_HEIGHT: f32 = 32.0;

#[allow(non_snake_case)]
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
            scope.items_indexed_rc(blocks.clone(), |_index, block| match block {
                MarkdownBlock::Text(annotated) => render_text_block(annotated.clone()),
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

#[allow(non_snake_case)]
#[composable]
fn render_markdown_blocks(blocks: Rc<[MarkdownBlock]>) {
    let list_state = remember_lazy_list_state();
    render_markdown_blocks_with_state(blocks, list_state);
}

#[allow(non_snake_case)]
#[composable]
fn render_markdown_blocks_with_state(
    blocks: Rc<[MarkdownBlock]>,
    list_state: cranpose_foundation::lazy::LazyListState,
) {
    let blocks_for_list = blocks.clone();
    LazyListWithScrollbar(
        Modifier::empty().fill_max_size(),
        list_state,
        "MarkdownScrollbarRail",
        markdown_scrollbar_style(),
        move || {
            MarkdownBlocksList(list_state, blocks_for_list.clone());
        },
    );
}

#[allow(non_snake_case)]
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
        // Inject the platform URI handler as the open_url callback.
        // LinkedText dispatches Url via open_url, Clickable handlers are called directly.
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

#[allow(non_snake_case)]
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::lazy_scrollbar::{
        average_visible_item_size, compute_scrollbar_metrics, compute_scrollbar_model,
        scroll_target_for_fraction, stabilize_scrollbar_model_for_scrollable_content,
        LazyScrollbarModel,
    };
    use cranpose_ui::text::FontWeight;

    #[test]
    fn default_url_points_to_leetcode_source_markdown() {
        assert_eq!(
            DEFAULT_URL,
            "https://raw.githubusercontent.com/samoylenkodmitry/s-a--m.github.io/refs/heads/master/_leetcode_source/2023-07-14-leetcode_daily.md"
        );
    }

    #[test]
    fn heading_produces_bold_block() {
        let blocks = markdown_to_blocks("# Hello World");
        assert_eq!(blocks.len(), 1);
        let MarkdownBlock::Text(annotated) = &blocks[0] else {
            panic!("expected Text block");
        };
        assert!(annotated.text.contains("Hello World"));
        let has_bold = annotated
            .span_styles
            .iter()
            .any(|s| s.item.font_weight == Some(FontWeight::BOLD));
        assert!(has_bold, "H1 should produce bold span style");
    }

    #[test]
    fn bold_inline_produces_bold_span() {
        let blocks = markdown_to_blocks("Normal **bold** normal");
        assert_eq!(blocks.len(), 1);
        let MarkdownBlock::Text(annotated) = &blocks[0] else {
            panic!("expected Text block");
        };
        let has_bold = annotated
            .span_styles
            .iter()
            .any(|s| s.item.font_weight == Some(FontWeight::BOLD));
        assert!(has_bold, "**bold** should produce a bold span");
    }

    #[test]
    fn italic_inline_produces_italic_span() {
        let blocks = markdown_to_blocks("Normal *italic* normal");
        assert_eq!(blocks.len(), 1);
        let MarkdownBlock::Text(annotated) = &blocks[0] else {
            panic!("expected Text block");
        };
        let has_italic = annotated
            .span_styles
            .iter()
            .any(|s| s.item.font_style == Some(FontStyle::Italic));
        assert!(has_italic, "*italic* should produce an italic span");
    }

    #[test]
    fn horizontal_rule_produces_rule_block() {
        let blocks = markdown_to_blocks("---");
        let has_rule = blocks.iter().any(|b| matches!(b, MarkdownBlock::Rule));
        assert!(has_rule, "--- should emit a Rule block");
    }

    #[test]
    fn empty_input_yields_no_blocks() {
        let blocks = markdown_to_blocks("");
        assert!(blocks.is_empty());
    }

    #[test]
    fn markdown_scroll_stress_fixture_is_large_representative_content() {
        let fixture = markdown_scroll_stress_fixture();
        assert!(fixture.starts_with("# Markdown Scroll Stress Fixture"));
        assert!(fixture.contains("Paragraph 419"));
        assert!(fixture.contains("### Section 408"));
        assert!(
            fixture.len() > 40_000,
            "stress fixture should stay large enough to exercise fetched Markdown scrolling"
        );
    }

    #[test]
    fn multiple_paragraphs_yield_separate_blocks() {
        let blocks = markdown_to_blocks("First paragraph\n\nSecond paragraph");
        let text_blocks: Vec<_> = blocks
            .iter()
            .filter(|b| matches!(b, MarkdownBlock::Text(_)))
            .collect();
        assert_eq!(
            text_blocks.len(),
            2,
            "expected two separate paragraph blocks"
        );
    }

    #[test]
    fn plain_paragraphs_do_not_emit_empty_span_styles() {
        let blocks = markdown_to_blocks("plain paragraph");
        assert_eq!(blocks.len(), 1);
        let MarkdownBlock::Text(annotated) = &blocks[0] else {
            panic!("expected Text block");
        };
        assert!(
            annotated.span_styles.is_empty(),
            "unstyled markdown should not force styled-text rendering"
        );
    }

    #[test]
    fn list_item_paragraph_keeps_bullet_and_text_in_same_block() {
        let blocks = markdown_to_blocks("- Time complexity: $$O(n)$$");
        assert_eq!(blocks.len(), 1, "single list item should produce one block");
        let MarkdownBlock::Text(annotated) = &blocks[0] else {
            panic!("expected Text block");
        };
        assert!(
            annotated.text.starts_with("• Time complexity:"),
            "bullet and text must stay in the same block"
        );
    }

    #[test]
    fn list_items_do_not_emit_bullet_only_blocks() {
        let blocks = markdown_to_blocks("- first\n- second");
        let text_blocks: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                MarkdownBlock::Text(annotated) => Some(annotated),
                MarkdownBlock::Rule => None,
            })
            .collect();
        assert_eq!(text_blocks.len(), 2, "expected one block per list item");
        assert!(
            text_blocks.iter().all(|item| item.text.trim() != "•"),
            "renderer emitted bullet-only block"
        );
    }

    #[test]
    fn link_stores_url_link_annotation() {
        let blocks = markdown_to_blocks("Click [here](https://example.com) please");
        assert_eq!(blocks.len(), 1);
        let MarkdownBlock::Text(annotated) = &blocks[0] else {
            panic!("expected Text block");
        };
        assert!(
            !annotated.link_annotations.is_empty(),
            "link should produce a LinkAnnotation"
        );
        let url_anns = annotated.get_link_annotations(0, annotated.text.len());
        assert_eq!(url_anns.len(), 1);
        assert!(
            matches!(&url_anns[0].item, LinkAnnotation::Url(url) if url == "https://example.com"),
            "expected LinkAnnotation::Url with the correct URL"
        );
    }

    #[test]
    fn link_annotation_covers_only_link_text() {
        let blocks = markdown_to_blocks("Before [link](https://x.com) after");
        let MarkdownBlock::Text(annotated) = &blocks[0] else {
            panic!("expected Text block");
        };
        // Find the byte range of "link" in the text
        let start = annotated.text.find("link").expect("link text present");
        let end = start + "link".len();
        let ann = &annotated.link_annotations[0];
        assert_eq!(
            ann.range,
            start..end,
            "link annotation should cover only 'link'"
        );
    }

    #[test]
    fn scrollbar_metrics_handle_small_rail_height() {
        let (thumb_h, thumb_y) = compute_scrollbar_metrics(16.0, 0.04, 1.0, 32.0);
        assert_eq!(thumb_h, 16.0);
        assert_eq!(thumb_y, 0.0);
    }

    #[test]
    fn scrollbar_metrics_clamp_scroll_fraction() {
        let (_, low_y) = compute_scrollbar_metrics(100.0, 0.5, -10.0, 32.0);
        let (_, high_y) = compute_scrollbar_metrics(100.0, 0.5, 10.0, 32.0);
        assert_eq!(low_y, 0.0);
        assert_eq!(high_y, 50.0);
    }

    #[test]
    fn scrollbar_model_computes_fraction_from_position() {
        let model = compute_scrollbar_model(100, 200.0, 20.0, 10, 10.0);
        assert_eq!(model.total_items, 100);
        assert!((model.max_item_position - 90.0).abs() < 0.001);
        assert!((model.thumb_fraction - 0.1).abs() < 0.001);
        assert!((model.scroll_fraction - (10.5 / 90.0)).abs() < 0.0001);
    }

    #[test]
    fn average_visible_item_size_prefers_measured_visible_items() {
        let layout = cranpose_foundation::lazy::LazyListLayoutInfo {
            visible_items_info: vec![
                cranpose_foundation::lazy::LazyListItemInfo {
                    index: 0,
                    key: 0,
                    offset: 0.0,
                    size: 20.0,
                },
                cranpose_foundation::lazy::LazyListItemInfo {
                    index: 1,
                    key: 1,
                    offset: 20.0,
                    size: 40.0,
                },
            ],
            ..Default::default()
        };

        let avg = average_visible_item_size(&layout, 100.0);
        assert!((avg - 30.0).abs() < 0.001);
    }

    #[test]
    fn stabilize_scrollbar_model_keeps_thumb_visible_when_scrollable() {
        let model = LazyScrollbarModel {
            total_items: 18,
            average_item_size: 32.0,
            max_item_position: 0.0,
            thumb_fraction: 1.0,
            scroll_fraction: 0.0,
        };

        let stabilized = stabilize_scrollbar_model_for_scrollable_content(model, true, false);
        assert!(stabilized.max_item_position > 0.0);
        assert!(stabilized.thumb_fraction < 1.0);
        assert_eq!(stabilized.scroll_fraction, 0.0);
    }

    #[test]
    fn stabilize_scrollbar_model_preserves_non_scrollable_model() {
        let model = LazyScrollbarModel {
            total_items: 5,
            average_item_size: 40.0,
            max_item_position: 0.0,
            thumb_fraction: 1.0,
            scroll_fraction: 0.0,
        };
        let stabilized = stabilize_scrollbar_model_for_scrollable_content(model, false, false);
        assert_eq!(stabilized, model);
    }

    #[test]
    fn scroll_target_for_fraction_maps_to_item_and_offset() {
        let model = compute_scrollbar_model(100, 200.0, 20.0, 0, 0.0);
        let (idx, off) = scroll_target_for_fraction(model, 0.5);
        assert_eq!(idx, 45);
        assert_eq!(off, 0.0);

        let (idx2, off2) = scroll_target_for_fraction(model, 0.5055556);
        assert_eq!(idx2, 45);
        assert!((off2 - 10.0).abs() < 0.001);
    }

    #[test]
    fn scroll_target_for_fraction_handles_non_scrollable_model() {
        let model = compute_scrollbar_model(3, 500.0, 50.0, 0, 0.0);
        assert_eq!(model.max_item_position, 0.0);
        let (idx, off) = scroll_target_for_fraction(model, 1.0);
        assert_eq!(idx, 0);
        assert_eq!(off, 0.0);
    }

    #[test]
    fn split_large_markdown_blocks_preserves_text_content() {
        let long = "a".repeat(MAX_MARKDOWN_BLOCK_BYTES * 2 + 100);
        let input = vec![MarkdownBlock::Text(Rc::new(AnnotatedString::from(
            long.as_str(),
        )))];
        let split = split_large_markdown_blocks(input);
        assert!(
            split.len() >= 2,
            "expected long text block to be split into multiple chunks"
        );
        let mut joined = String::new();
        for block in &split {
            let MarkdownBlock::Text(annotated) = block else {
                continue;
            };
            assert!(
                annotated.text.len() <= MAX_MARKDOWN_BLOCK_BYTES,
                "chunk exceeded max block size"
            );
            joined.push_str(&annotated.text);
        }
        assert_eq!(joined, long, "splitting must preserve full text");
    }

    #[test]
    fn split_large_markdown_blocks_preserves_links() {
        let repeated = format!(
            "{} [link](https://example.com) {}",
            "x".repeat(MAX_MARKDOWN_BLOCK_BYTES),
            "y".repeat(MAX_MARKDOWN_BLOCK_BYTES)
        );
        let split = split_large_markdown_blocks(markdown_to_blocks(&repeated));
        let link_count = split
            .iter()
            .filter_map(|block| match block {
                MarkdownBlock::Text(annotated) => Some(annotated.link_annotations.len()),
                MarkdownBlock::Rule => None,
            })
            .sum::<usize>();
        assert_eq!(
            link_count, 1,
            "link annotations should be preserved after split"
        );
    }

    #[test]
    fn markdown_scroll_stress_fixture_exercises_many_rendered_blocks() {
        let markdown = markdown_scroll_stress_fixture();
        let blocks = split_large_markdown_blocks(markdown_to_blocks(&markdown));
        let text_blocks = blocks
            .iter()
            .filter(|block| matches!(block, MarkdownBlock::Text(_)))
            .count();

        assert!(markdown.len() > 60_000);
        assert!(text_blocks >= 420);
        assert!(
            blocks.iter().any(|block| match block {
                MarkdownBlock::Text(annotated) => !annotated.link_annotations.is_empty(),
                MarkdownBlock::Rule => false,
            }),
            "stress fixture must include linked text"
        );
    }

    #[test]
    fn markdown_code_blocks_drop_fence_terminator_newlines() {
        let blocks = markdown_to_blocks(
            "```kotlin\nfun a() {\n    println(1)\n}\n```\n```rust\nfn b() {}\n```\n",
        );
        let texts = blocks
            .iter()
            .filter_map(|block| match block {
                MarkdownBlock::Text(annotated) => Some(annotated.text.as_str()),
                MarkdownBlock::Rule => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(texts.len(), 2);
        assert_eq!(texts[0], "fun a() {\n    println(1)\n}");
        assert_eq!(texts[1], "fn b() {}");
    }

    #[test]
    #[ignore = "profiling helper: run manually with MD_PROFILE_PATH=/path/to/file"]
    fn profile_large_markdown_from_file() {
        use std::time::Instant;

        let path = std::env::var("MD_PROFILE_PATH")
            .expect("set MD_PROFILE_PATH to a markdown file for profiling");
        let markdown = std::fs::read_to_string(&path).expect("failed to read markdown file");
        let bytes = markdown.len();

        let started = Instant::now();
        let blocks = markdown_to_blocks(&markdown);
        let elapsed = started.elapsed();

        let mut text_block_count = 0usize;
        let mut max_block_bytes = 0usize;
        let mut max_block_preview = String::new();
        for block in &blocks {
            if let MarkdownBlock::Text(annotated) = block {
                text_block_count += 1;
                if annotated.text.len() > max_block_bytes {
                    max_block_bytes = annotated.text.len();
                    max_block_preview = annotated.text.chars().take(120).collect();
                }
            }
        }

        println!("PROFILE_MD: file={path}");
        println!("PROFILE_MD: input_bytes={bytes}");
        println!("PROFILE_MD: total_blocks={}", blocks.len());
        println!("PROFILE_MD: text_blocks={text_block_count}");
        println!("PROFILE_MD: max_block_bytes={max_block_bytes}");
        println!("PROFILE_MD: max_block_preview={max_block_preview:?}");
        println!("PROFILE_MD: parse_ms={:.2}", elapsed.as_secs_f64() * 1000.0);
    }
}
