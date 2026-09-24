use cranpose_ui::text::FontWeight;

use super::*;
use crate::app::lazy_scrollbar::{
    average_visible_item_size, compute_scrollbar_metrics, compute_scrollbar_model,
    scroll_target_for_fraction, stabilize_scrollbar_model_for_scrollable_content,
    LazyScrollbarModel,
};

#[test]
fn default_url_points_to_leetcode_source_markdown() {
    assert_eq!(
        DEFAULT_URL,
        "https://raw.githubusercontent.com/samoylenkodmitry/s-a--m.github.io/refs/heads/master/_leetcode_source/2023-07-14-leetcode_daily.md"
    );
}

#[test]
fn heading_produces_bold_block() {
    let blocks = markdown_to_blocks("# Hello World", "");
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
    let blocks = markdown_to_blocks("Normal **bold** normal", "");
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
    let blocks = markdown_to_blocks("Normal *italic* normal", "");
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
    let blocks = markdown_to_blocks("---", "");
    let has_rule = blocks.iter().any(|b| matches!(b, MarkdownBlock::Rule));
    assert!(has_rule, "--- should emit a Rule block");
}

#[test]
fn empty_input_yields_no_blocks() {
    let blocks = markdown_to_blocks("", "");
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
    let blocks = markdown_to_blocks("First paragraph\n\nSecond paragraph", "");
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
    let blocks = markdown_to_blocks("plain paragraph", "");
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
    let blocks = markdown_to_blocks("- Time complexity: $$O(n)$$", "");
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
    let blocks = markdown_to_blocks("- first\n- second", "");
    let text_blocks: Vec<_> = blocks
        .iter()
        .filter_map(|block| match block {
            MarkdownBlock::Text(annotated) => Some(annotated),
            MarkdownBlock::Image { .. } => None,
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
    let blocks = markdown_to_blocks("Click [here](https://example.com) please", "");
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
    let blocks = markdown_to_blocks("Before [link](https://x.com) after", "");
    let MarkdownBlock::Text(annotated) = &blocks[0] else {
        panic!("expected Text block");
    };
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
    let split = split_large_markdown_blocks(markdown_to_blocks(&repeated, ""));
    let link_count = split
        .iter()
        .filter_map(|block| match block {
            MarkdownBlock::Text(annotated) => Some(annotated.link_annotations.len()),
            MarkdownBlock::Image { .. } => None,
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
    let blocks = split_large_markdown_blocks(markdown_to_blocks(&markdown, ""));
    let text_blocks = blocks
        .iter()
        .filter(|block| matches!(block, MarkdownBlock::Text(_)))
        .count();

    assert!(markdown.len() > 60_000);
    assert!(text_blocks >= 420);
    assert!(
        blocks.iter().any(|block| match block {
            MarkdownBlock::Text(annotated) => !annotated.link_annotations.is_empty(),
            MarkdownBlock::Image { .. } => false,
            MarkdownBlock::Rule => false,
        }),
        "stress fixture must include linked text"
    );
}

#[test]
fn markdown_code_blocks_drop_fence_terminator_newlines() {
    let blocks = markdown_to_blocks(
        "```kotlin\nfun a() {\n    println(1)\n}\n```\n```rust\nfn b() {}\n```\n",
        "",
    );
    let texts = blocks
        .iter()
        .filter_map(|block| match block {
            MarkdownBlock::Text(annotated) => Some(annotated.text.as_str()),
            MarkdownBlock::Image { .. } => None,
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
    let blocks = markdown_to_blocks(&markdown, "");
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

#[test]
fn an_image_becomes_its_own_block_instead_of_placeholder_text() {
    let blocks = markdown_to_blocks("![a cat](https://example.com/cat.png)", "");
    let images: Vec<_> = blocks
        .iter()
        .filter_map(|block| match block {
            MarkdownBlock::Image { url, alt } => Some((url.as_str(), alt.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(images, vec![("https://example.com/cat.png", "a cat")]);
    for block in &blocks {
        if let MarkdownBlock::Text(annotated) = block {
            assert!(
                !annotated.text.contains("[image:"),
                "the placeholder text survived: {:?}",
                annotated.text
            );
        }
    }
}

#[test]
fn an_image_without_alt_text_still_produces_a_block() {
    let blocks = markdown_to_blocks("![](https://example.com/x.png)", "");
    assert!(blocks
        .iter()
        .any(|block| matches!(block, MarkdownBlock::Image { url, .. }
            if url == "https://example.com/x.png")));
}

#[test]
fn a_root_relative_image_resolves_against_the_document_url() {
    let blocks = markdown_to_blocks(
        "![day](/assets/day.webp)",
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/notes/post.md",
    );
    let urls: Vec<&str> = blocks
        .iter()
        .filter_map(|block| match block {
            MarkdownBlock::Image { url, .. } => Some(url.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        urls,
        vec!["https://raw.githubusercontent.com/owner/repo/refs/heads/master/assets/day.webp"],
        "a root-relative image must become a fetchable absolute URL"
    );
}

#[test]
fn a_root_relative_link_resolves_against_the_document_url() {
    let blocks = markdown_to_blocks(
        "see [notes](/leetcode/)",
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/notes/post.md",
    );
    let has_absolute_link = blocks.iter().any(|block| match block {
        MarkdownBlock::Text(annotated) => annotated.link_annotations.iter().any(|span| {
            matches!(&span.item, LinkAnnotation::Url(url)
                if url == "https://raw.githubusercontent.com/owner/repo/refs/heads/master/leetcode/")
        }),
        _ => false,
    });
    assert!(has_absolute_link, "a root-relative link stayed relative");
}

#[test]
fn an_image_with_an_empty_url_is_dropped() {
    let blocks = markdown_to_blocks("![alt]()", "");
    assert!(!blocks
        .iter()
        .any(|block| matches!(block, MarkdownBlock::Image { .. })));
}

fn code_block_text(markdown: &str) -> Rc<AnnotatedString> {
    markdown_to_blocks(markdown, "")
        .into_iter()
        .find_map(|block| match block {
            MarkdownBlock::Text(annotated) if annotated.text.contains("fn ") => Some(annotated),
            _ => None,
        })
        .expect("a code block")
}

#[test]
fn a_rust_fence_colours_its_keywords() {
    let annotated = code_block_text(
        "```rust
fn main() {}
```",
    );
    assert_eq!(annotated.text, "fn main() {}");
    let coloured = annotated
        .span_styles
        .iter()
        .filter(|span| span.item.color.is_some())
        .count();
    assert!(
        coloured > 0,
        "expected coloured spans in a rust fence: {:?}",
        annotated.span_styles
    );
}

#[test]
fn a_kotlin_fence_is_coloured() {
    let annotated = markdown_to_blocks("```kotlin\nfun main() { val x = \"hi\" }\n```", "")
        .into_iter()
        .find_map(|block| match block {
            MarkdownBlock::Text(annotated) if annotated.text.contains("fun ") => Some(annotated),
            _ => None,
        })
        .expect("a kotlin code block");
    let coloured = annotated
        .span_styles
        .iter()
        .filter(|span| span.item.color.is_some())
        .count();
    assert!(
        coloured >= 2,
        "kotlin code reached the screen uncoloured: {coloured} coloured spans"
    );
}

#[test]
fn an_unfenced_code_block_keeps_its_text_uncoloured() {
    let annotated = code_block_text(
        "```
fn main() {}
```",
    );
    assert_eq!(annotated.text, "fn main() {}");
    let coloured = annotated
        .span_styles
        .iter()
        .filter(|span| span.item.color.is_some())
        .count();
    assert_eq!(coloured, 0, "a fence with no language must not be coloured");
}

#[test]
fn a_fence_preserves_its_code_exactly() {
    let code = "let s = \"héllo\";\nlet n = 0xff;";
    let markdown = format!("```rust\n{code}\n```");
    let annotated = markdown_to_blocks(&markdown, "")
        .into_iter()
        .find_map(|block| match block {
            MarkdownBlock::Text(annotated) if annotated.text.contains("let") => Some(annotated),
            _ => None,
        })
        .expect("a code block");
    assert_eq!(annotated.text, code);
}
