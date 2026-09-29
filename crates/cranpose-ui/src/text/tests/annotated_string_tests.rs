use super::*;

#[test]
fn a_redrawn_string_reuses_the_annotated_copy_from_last_frame() {
    let first = shared_plain_annotated_string("SCORE 340");
    let second = shared_plain_annotated_string("SCORE 340");
    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(second.text, "SCORE 340");
    assert!(second.span_styles.is_empty());
}

#[test]
fn distinct_strings_never_share_an_annotated_copy() {
    let first = shared_plain_annotated_string("READY");
    let second = shared_plain_annotated_string("GO");
    assert!(!Rc::ptr_eq(&first, &second));
    assert_eq!(first.text, "READY");
    assert_eq!(second.text, "GO");
}

#[test]
fn the_pool_survives_overflowing_its_capacity() {
    for index in 0..600 {
        let text = format!("distinct-{index}");
        let shared = shared_plain_annotated_string(&text);
        assert_eq!(shared.text, text);
    }
    let after = shared_plain_annotated_string("still correct");
    assert_eq!(after.text, "still correct");
}

#[test]
fn test_builder_span() {
    let span1 = SpanStyle {
        alpha: Some(0.5),
        ..Default::default()
    };

    let span2 = SpanStyle {
        alpha: Some(1.0),
        ..Default::default()
    };

    let annotated = AnnotatedString::builder()
        .append("Hello ")
        .push_style(span1.clone())
        .append("World")
        .push_style(span2.clone())
        .append("!")
        .pop()
        .pop()
        .to_annotated_string();

    assert_eq!(annotated.text, "Hello World!");
    assert_eq!(annotated.span_styles.len(), 2);
    assert_eq!(annotated.span_styles[0].range, 6..12);
    assert_eq!(annotated.span_styles[0].item, span1);
    assert_eq!(annotated.span_styles[1].range, 11..12);
    assert_eq!(annotated.span_styles[1].item, span2);
}

#[test]
fn with_link_url_roundtrips() {
    let url = "https://developer.android.com";
    let annotated = AnnotatedString::builder()
        .append("Visit ")
        .with_link(LinkAnnotation::Url(url.into()), |b| {
            b.append("Android Developers")
        })
        .append(".")
        .to_annotated_string();

    assert_eq!(annotated.text, "Visit Android Developers.");
    assert_eq!(annotated.link_annotations.len(), 1);
    let ann = &annotated.link_annotations[0];
    assert_eq!(ann.range, 6..24);
    assert_eq!(ann.item, LinkAnnotation::Url(url.into()));
}

#[test]
fn with_link_clickable_calls_handler() {
    use std::cell::Cell;
    let called = Rc::new(Cell::new(false));
    let called_clone = Rc::clone(&called);

    let annotated = AnnotatedString::builder()
        .with_link(
            LinkAnnotation::Clickable {
                tag: "action".into(),
                handler: Rc::new(move || called_clone.set(true)),
            },
            |b| b.append("click me"),
        )
        .to_annotated_string();

    assert_eq!(annotated.link_annotations.len(), 1);
    let ann = &annotated.link_annotations[0];
    if let LinkAnnotation::Clickable { handler, .. } = &ann.item {
        handler();
    }
    assert!(called.get(), "Clickable handler should have been called");
}

#[test]
fn with_link_subsequence_trims_range() {
    let annotated = AnnotatedString::builder()
        .append("pre ")
        .with_link(LinkAnnotation::Url("http://x.com".into()), |b| {
            b.append("link")
        })
        .append(" post")
        .to_annotated_string();

    let sub = annotated.subsequence(4..8);
    assert_eq!(sub.link_annotations.len(), 1);
    assert_eq!(sub.link_annotations[0].range, 0..4);
}

#[test]
fn append_annotated_preserves_ranges_with_existing_prefix() {
    let annotated = AnnotatedString::builder()
        .append("Hello ")
        .push_style(SpanStyle {
            alpha: Some(0.5),
            ..Default::default()
        })
        .append("World")
        .pop()
        .push_string_annotation("kind", "planet")
        .append("!")
        .pop()
        .to_annotated_string();

    let combined = AnnotatedString::builder()
        .append("Prefix ")
        .append_annotated(&annotated)
        .to_annotated_string();

    assert_eq!(combined.text, "Prefix Hello World!");
    assert_eq!(combined.span_styles.len(), 1);
    assert_eq!(combined.span_styles[0].range, 13..18);
    assert_eq!(combined.string_annotations.len(), 1);
    assert_eq!(combined.string_annotations[0].range, 18..19);
}

#[test]
fn append_annotated_subsequence_clips_ranges_to_slice() {
    let annotated = AnnotatedString::builder()
        .append("Before ")
        .push_style(SpanStyle {
            alpha: Some(0.5),
            ..Default::default()
        })
        .append("Styled")
        .pop()
        .with_link(LinkAnnotation::Url("https://example.com".into()), |b| {
            b.append(" Link")
        })
        .to_annotated_string();

    let slice = AnnotatedString::builder()
        .append("-> ")
        .append_annotated_subsequence(&annotated, 7..18)
        .to_annotated_string();

    assert_eq!(slice.text, "-> Styled Link");
    assert_eq!(slice.span_styles.len(), 1);
    assert_eq!(slice.span_styles[0].range, 3..9);
    assert_eq!(slice.link_annotations.len(), 1);
    assert_eq!(slice.link_annotations[0].range, 9..14);
}

#[test]
fn render_hash_changes_for_visual_style_ranges() {
    let plain = AnnotatedString::builder()
        .append("Hello")
        .to_annotated_string();
    let styled = AnnotatedString::builder()
        .push_style(SpanStyle {
            color: Some(crate::modifier::Color(1.0, 0.0, 0.0, 1.0)),
            ..Default::default()
        })
        .append("Hello")
        .pop()
        .to_annotated_string();

    assert_ne!(plain.render_hash(), styled.render_hash());
}

#[test]
fn a_redrawn_string_reuses_the_render_copy_from_last_frame() {
    let first = shared_plain_render_string("SCORE 341");
    let second = shared_plain_render_string("SCORE 341");
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(second.text, "SCORE 341");
    assert_eq!(
        second.text,
        shared_plain_annotated_string("SCORE 341").text,
        "both copies hold the same characters"
    );
}

#[test]
fn a_render_string_keeps_the_hash_its_content_has() {
    let annotated = AnnotatedString::builder()
        .append("plain ")
        .push_style(SpanStyle {
            color: Some(cranpose_ui_graphics::Color::RED),
            ..Default::default()
        })
        .append("red")
        .pop()
        .to_annotated_string();
    let render = annotated.render_string();
    assert_eq!(render.render_hash(), annotated.render_hash());
    assert_eq!(render.text(), "plain red");
    assert_eq!(render.span_styles(), annotated.span_styles.as_slice());
    let tail = render.subsequence(6..9);
    assert_eq!(tail.text(), "red");
    assert_eq!(
        tail.render_hash(),
        annotated.subsequence(6..9).render_hash(),
        "a subsequence hashes as a string built with that content"
    );
    assert_eq!(
        RenderString::default().render_hash(),
        AnnotatedString::from("").render_hash()
    );
    assert_eq!(render.subsequence(3..3), RenderString::default());
}

#[test]
fn span_boundaries_are_sorted_unique_character_offsets_held_inline_for_few_spans() {
    let span = |range: Range<usize>| RangeStyle {
        item: SpanStyle::default(),
        range,
    };
    let text = AnnotatedString {
        text: "héllo world".to_string(),
        span_styles: vec![span(6..12), span(2..40), span(2..3)],
        ..Default::default()
    };

    let boundaries = text.span_boundaries();

    // 2 is inside `é` and 40 is past the end: neither is a place styles
    // can change.
    assert_eq!(boundaries.as_slice(), &[0, 3, 6, 12]);
    assert!(!boundaries.spilled());
    assert_eq!(
        text.render_string().span_boundaries().as_slice(),
        boundaries.as_slice()
    );
    assert_eq!(AnnotatedString::from("").span_boundaries().as_slice(), &[0]);
}
