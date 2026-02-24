use cranpose_foundation::lazy::remember_lazy_list_state;
use cranpose_foundation::text::TextFieldState;
use cranpose_services::{local_http_client, local_uri_handler, HttpClientRef};
use cranpose_ui::{
    composable,
    text::{
        AnnotatedString, FontFamily, FontStyle, FontWeight, LinkAnnotation, SpanStyle,
        TextDecoration, TextUnit,
    },
    Brush, Button, Color, Column, ColumnSpec, CornerRadii, LazyColumn, LazyColumnSpec,
    LinearArrangement, LinkedText, Modifier, Row, RowSpec, Size, Spacer, Text, TextStyle,
    VerticalAlignment,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

// ---------------------------------------------------------------------------
// Markdown → AnnotatedString block list
// ---------------------------------------------------------------------------

/// One rendered "block" of markdown content.
#[derive(Clone, Debug, PartialEq)]
enum MarkdownBlock {
    /// A styled paragraph of inline text (may contain link annotations for clickable links).
    Text(AnnotatedString),
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
}

impl BlockBuilder {
    fn new() -> Self {
        Self {
            style: InlineStyle::default(),
            builder_raw: None,
            blocks: Vec::new(),
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
        self.builder_raw = Some(b.push_style(style));
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

    fn flush_block(&mut self) {
        if let Some(b) = self.builder_raw.take() {
            let s = b.to_annotated_string();
            if !s.text.is_empty() {
                self.blocks.push(MarkdownBlock::Text(s));
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
            Event::Start(Tag::Paragraph) => {
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
                b.push_inline_style();
            }
            Event::Start(Tag::Item) => {
                b.flush_block();
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
            Event::Text(text) => b.append(&text),
            Event::SoftBreak => b.append(" "),
            Event::HardBreak => b.append("\n"),
            Event::Rule => b.push_rule(),

            // ---- Block end ----
            Event::End(TagEnd::Heading(_)) => {
                b.pop_style();
                b.style.heading = None;
                b.flush_block();
            }
            Event::End(TagEnd::Paragraph) => {
                b.pop_style();
                b.flush_block();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                b.pop_style();
                b.style.blockquote_depth = b.style.blockquote_depth.saturating_sub(1);
                b.flush_block();
            }
            Event::End(TagEnd::CodeBlock) => {
                b.pop_style();
                b.style.code = false;
                b.flush_block();
            }
            Event::End(TagEnd::Item) => {
                b.pop_style();
                b.flush_block();
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

// ---------------------------------------------------------------------------
// Fetch state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum FetchState {
    Idle,
    Loading,
    Done(Vec<MarkdownBlock>),
    Error(String),
}

async fn fetch_markdown(client: &HttpClientRef, url: &str) -> Result<String, String> {
    client
        .get_text(url)
        .await
        .map_err(|e| format!("Request failed: {e}"))
}

const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/samoylenkodmitry/s-a--m.github.io/refs/heads/master/_posts/2023-07-14-leetcode_daily.md";

// ---------------------------------------------------------------------------
// Composable
// ---------------------------------------------------------------------------

#[allow(non_snake_case)]
#[composable]
pub fn markdown_viewer_tab() {
    let url_state =
        cranpose_core::remember(|| TextFieldState::new(DEFAULT_URL)).with(|s| s.clone());
    let fetch_state = cranpose_core::useState(|| FetchState::Idle);
    let request_counter = cranpose_core::useState(|| 0u64);
    let http_client = local_http_client().current();

    let url_state_for_effect = url_state.clone();
    cranpose_core::LaunchedEffect!(request_counter.get(), move |scope| {
        let tick = request_counter.get();
        if tick == 0 {
            return;
        }

        let url = url_state_for_effect.text();
        let url = url.trim().to_string();
        let status = fetch_state;
        status.set(FetchState::Loading);

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
                    let blocks = markdown_to_blocks(&text);
                    status.set(FetchState::Done(blocks));
                }
                Err(err) => status.set(FetchState::Error(err)),
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
            let url_state_for_row = url_state.clone();
            let request_state = request_counter;
            let status_state = fetch_state;
            move || {
                // ---- URL input row ----
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let url_row = url_state_for_row.clone();
                        let req = request_state;
                        move || {
                            cranpose_ui::BasicTextField(
                                url_row.clone(),
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
                                move || req.update(|v| *v = v.wrapping_add(1)),
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
                match status_state.get() {
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

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

#[allow(non_snake_case)]
#[composable]
fn render_markdown_blocks(blocks: Vec<MarkdownBlock>) {
    let list_state = remember_lazy_list_state();

    // Read reactive scroll position — triggers recomposition on scroll.
    let first_idx = list_state.first_visible_item_index();
    let first_offset = list_state.first_visible_item_scroll_offset();
    let can_scroll = list_state.can_scroll_forward() || list_state.can_scroll_backward();

    // Compute thumb geometry from layout info.
    let layout_info = list_state.layout_info();
    let avg_size = list_state.average_item_size().max(1.0);
    let total_items = layout_info.total_items_count;
    let viewport = layout_info.viewport_size;
    let total_est = (total_items as f32 * avg_size).max(viewport + 1.0);
    let scroll_pos = first_idx as f32 * avg_size + first_offset;

    const RAIL_W: f32 = 16.0;
    const THUMB_W: f32 = 8.0;
    const MIN_THUMB_H: f32 = 32.0;

    // Row: [ LazyColumn (weight=1) | ScrollbarRail (RAIL_W) ]
    Row(
        Modifier::empty().fill_max_size(),
        RowSpec::new(),
        {
            let blocks_for_row = blocks.clone();
            move || {
                // ---- Content list ----------------------------------------
                let blocks_for_list = blocks_for_row.clone();
                LazyColumn(
                    Modifier::empty().weight(1.0).fill_max_height(),
                    list_state,
                    LazyColumnSpec::new()
                        .vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                    move |scope| {
                        use cranpose_foundation::lazy::LazyListScopeExt;
                        scope.items_vec(blocks_for_list.clone(), |block| match block.clone() {
                            MarkdownBlock::Text(annotated) => render_text_block(annotated),
                            MarkdownBlock::Rule => render_rule(),
                        });
                    },
                );

                // ---- Scrollbar rail --------------------------------------
                // The thumb_frac and scroll_frac drive both the drawing and the
                // hit-test at gesture start. They are captured from composition
                // (reactive), so the draw refreshes on every scroll event.
                let thumb_frac = (viewport / total_est).clamp(0.04, 1.0);
                let max_scroll = total_est - viewport;
                let scroll_frac = if max_scroll > 0.0 {
                    (scroll_pos / max_scroll).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                cranpose_ui::Box(
                    Modifier::empty()
                        .width(RAIL_W)
                        .fill_max_height()
                        // Solid background so the rail is always visible
                        .background(Color(0.12, 0.15, 0.24, 1.0))
                        // ---- Visual: track + thumb ----
                        .draw_behind(move |scope| {
                            let h = scope.size().height.max(1.0);
                            // Always draw — thumb fills full height when content fits.
                            let thumb_h = (thumb_frac * h).max(MIN_THUMB_H).min(h);
                            let thumb_y =
                                (scroll_frac * (h - thumb_h)).clamp(0.0, h - thumb_h);
                            let x = (RAIL_W - THUMB_W) * 0.5;
                            scope.draw_rect_at(
                                cranpose_ui::Rect {
                                    x,
                                    y: thumb_y,
                                    width: THUMB_W,
                                    height: thumb_h,
                                },
                                Brush::solid(Color(0.55, 0.68, 1.0, 0.90)),
                            );
                        })
                        // ---- Interaction: drag to scroll ----
                        .pointer_input("scrollbar_drag", move |scope| async move {
                            use cranpose_foundation::PointerEventKind;
                            loop {
                                // One gesture: Down → Move* → Up/Cancel
                                scope
                                    .await_pointer_event_scope(|scope| async move {
                                        let mut dragging = false;
                                        let mut last_y = 0.0f32;
                                        loop {
                                            let event = scope.await_pointer_event().await;
                                            let h = scope.size().height;
                                            match event.kind {
                                                PointerEventKind::Down => {
                                                    // Re-compute thumb position using
                                                    // current (non-reactive) state.
                                                    let info = list_state.layout_info();
                                                    let avg = list_state
                                                        .average_item_size()
                                                        .max(1.0);
                                                    let vp = info.viewport_size.max(1.0);
                                                    let tot = (info.total_items_count
                                                        as f32
                                                        * avg)
                                                        .max(vp + 1.0);
                                                    let t_frac =
                                                        (vp / tot).clamp(0.04, 1.0);
                                                    let t_h =
                                                        (t_frac * h).max(MIN_THUMB_H);
                                                    let fidx = list_state
                                                        .first_visible_item_index();
                                                    let foff = list_state
                                                        .first_visible_item_scroll_offset();
                                                    let sp = fidx as f32 * avg + foff;
                                                    let ms = tot - vp;
                                                    let sf = if ms > 0.0 {
                                                        (sp / ms).clamp(0.0, 1.0)
                                                    } else {
                                                        0.0
                                                    };
                                                    let t_y = (sf * (h - t_h))
                                                        .clamp(0.0, h - t_h);
                                                    let y = event.position.y;
                                                    if y >= t_y && y <= t_y + t_h {
                                                        dragging = true;
                                                        last_y = y;
                                                    }
                                                }
                                                PointerEventKind::Move if dragging => {
                                                    let y = event.position.y;
                                                    let dy = y - last_y;
                                                    last_y = y;
                                                    if dy.abs() > 0.5 {
                                                        let info =
                                                            list_state.layout_info();
                                                        let avg = list_state
                                                            .average_item_size()
                                                            .max(1.0);
                                                        let vp =
                                                            info.viewport_size.max(1.0);
                                                        let tot = (info
                                                            .total_items_count
                                                            as f32
                                                            * avg)
                                                            .max(vp + 1.0);
                                                        let t_frac =
                                                            (vp / tot).clamp(0.04, 1.0);
                                                        let track_h =
                                                            (h - (t_frac * h)
                                                                .max(MIN_THUMB_H))
                                                            .max(1.0);
                                                        // dy in rail px → content px
                                                        let delta =
                                                            dy * (tot - vp) / track_h;
                                                        list_state
                                                            .dispatch_scroll_delta(delta);
                                                    }
                                                }
                                                PointerEventKind::Up
                                                | PointerEventKind::Cancel => {
                                                    dragging = false;
                                                    break; // end gesture, restart loop
                                                }
                                                _ => {}
                                            }
                                        }
                                    })
                                    .await;
                            }
                        }),
                    cranpose_ui::BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn render_text_block(annotated: AnnotatedString) {
    let text_style = TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.88, 0.90, 0.96, 1.0)),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    };

    if !annotated.link_annotations.is_empty() {
        // Inject the platform URI handler as the open_url callback.
        // LinkedText dispatches Url via open_url, Clickable handlers are called directly.
        let uri_handler = local_uri_handler().current();
        LinkedText(
            annotated,
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
    use cranpose_ui::text::FontWeight;

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
}
