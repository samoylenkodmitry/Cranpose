use std::rc::Rc;

use cranpose_ui::{
    AppContext, TextLayoutOptions, TextStyle, prepare_text_layout,
    text::{AnnotatedString, LinkAnnotation, LinkKey},
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
            prepared.text.string_annotations()[0].item.annotation,
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

        assert_eq!(
            prepared.text.links()[0].item,
            LinkKey::Url("https://example.invalid/second".into())
        );
    });
}

#[test]
fn prepared_cache_keeps_link_tag_for_equal_text_and_style() {
    AppContext::new().enter(|| {
        let clickable = |tag: &str| LinkAnnotation::Clickable {
            tag: tag.into(),
            handler: Rc::new(|| {}),
        };
        let first = annotated_with_link("payload", clickable("first-tag"));
        let second = annotated_with_link("payload", clickable("second-tag"));

        let _ = prepare(&first);
        let prepared = prepare(&second);

        assert_eq!(
            prepared.text.links()[0].item,
            LinkKey::Clickable("second-tag".into())
        );
    });
}
