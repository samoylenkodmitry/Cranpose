use std::{cell::Cell, rc::Rc};

use cranpose_ui::{
    AppContext, TextLayoutOptions, TextStyle, prepare_text_layout,
    text::{AnnotatedString, LinkAnnotation},
};

fn annotated_with_link(annotation: &str, link: LinkAnnotation) -> AnnotatedString {
    AnnotatedString::builder()
        .push_string_annotation("payload", annotation)
        .push_link(link)
        .append("tap")
        .pop()
        .pop()
        .to_annotated_string()
}

fn prepare(text: &AnnotatedString) -> Rc<cranpose_ui::PreparedTextLayout> {
    prepare_text_layout(
        text,
        &TextStyle::default(),
        TextLayoutOptions::default(),
        None,
    )
}

#[test]
fn prepared_cache_keeps_string_annotation_values_for_equal_text_and_style() {
    AppContext::new().enter(|| {
        let first = annotated_with_link(
            "first payload",
            LinkAnnotation::Url("https://example.invalid/first".into()),
        );
        let second = annotated_with_link(
            "second payload",
            LinkAnnotation::Url("https://example.invalid/second".into()),
        );

        let _ = prepare(&first);
        let prepared = prepare(&second);

        assert_eq!(
            prepared.text.string_annotations[0].item.annotation,
            "second payload"
        );
    });
}

#[test]
fn prepared_cache_keeps_url_identity_for_equal_text_and_style() {
    AppContext::new().enter(|| {
        let first = annotated_with_link(
            "payload",
            LinkAnnotation::Url("https://example.invalid/first".into()),
        );
        let second = annotated_with_link(
            "payload",
            LinkAnnotation::Url("https://example.invalid/second".into()),
        );

        let _ = prepare(&first);
        let prepared = prepare(&second);

        assert!(matches!(
            &prepared.text.link_annotations[0].item,
            LinkAnnotation::Url(url) if url == "https://example.invalid/second"
        ));
    });
}

#[test]
fn prepared_cache_keeps_click_handler_for_equal_link_tag() {
    AppContext::new().enter(|| {
        let first_calls = Rc::new(Cell::new(0));
        let first_calls_for_handler = Rc::clone(&first_calls);
        let first = annotated_with_link(
            "payload",
            LinkAnnotation::Clickable {
                tag: "same-tag".into(),
                handler: Rc::new(move || {
                    first_calls_for_handler.set(first_calls_for_handler.get() + 1);
                }),
            },
        );

        let second_calls = Rc::new(Cell::new(0));
        let second_calls_for_handler = Rc::clone(&second_calls);
        let second = annotated_with_link(
            "payload",
            LinkAnnotation::Clickable {
                tag: "same-tag".into(),
                handler: Rc::new(move || {
                    second_calls_for_handler.set(second_calls_for_handler.get() + 1);
                }),
            },
        );

        let _ = prepare(&first);
        let prepared = prepare(&second);
        if let Some(link) = prepared.text.link_annotations.first()
            && let LinkAnnotation::Clickable { handler, .. } = &link.item
        {
            handler();
        }

        assert_eq!(first_calls.get(), 0);
        assert_eq!(second_calls.get(), 1);
    });
}
