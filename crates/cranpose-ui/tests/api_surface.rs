use cranpose_ui::{
    ParagraphStyle, ScrollMetrics, ScrollState, run_test_composition,
    text::{AnnotatedString, LinkAnnotation},
    widgets::swipe_to_dismiss::{SwipeDismissDirection, SwipeToDismissSpec},
};

#[test]
fn scroll_metrics_add_the_viewport_to_what_is_left_to_travel() {
    let metrics = ScrollMetrics {
        offset: 10.0,
        max_offset: 90.0,
        viewport_extent: 100.0,
    };
    assert_eq!(metrics.content_extent(), 190.0);

    let fits = ScrollMetrics {
        offset: 0.0,
        max_offset: 0.0,
        viewport_extent: 100.0,
    };
    assert_eq!(fits.content_extent(), 100.0);
}

#[test]
fn a_fresh_scroll_state_has_no_measured_extents_and_no_settle_policy() {
    run_test_composition(|| {
        let state = ScrollState::new(0.0);
        assert_eq!(
            state.viewport_extent(),
            0.0,
            "a state nothing measured reported a viewport"
        );
        assert_eq!(state.content_extent(), 0.0);
        assert!(
            state.settle_policy().is_none(),
            "a policy appeared without being installed"
        );
    });
}

#[test]
fn a_swipe_spec_records_every_option_it_is_given() {
    let plain = SwipeToDismissSpec::default();
    assert_eq!(plain.direction, SwipeDismissDirection::Both);
    assert!(plain.collapse_after_dismiss);
    assert!(plain.edge_width.is_none());

    let configured = SwipeToDismissSpec::default()
        .with_threshold_fraction(0.25)
        .with_direction(SwipeDismissDirection::StartToEnd)
        .with_collapse_after_dismiss(false)
        .from_edge(24.0);
    assert_eq!(configured.threshold_fraction, 0.25);
    assert_eq!(configured.direction, SwipeDismissDirection::StartToEnd);
    assert!(!configured.collapse_after_dismiss);
    assert_eq!(configured.edge_width, Some(24.0));
}

#[test]
fn a_swipe_edge_width_is_never_negative() {
    assert_eq!(
        SwipeToDismissSpec::default().from_edge(-8.0).edge_width,
        Some(0.0)
    );
}

#[test]
fn an_annotated_string_carries_the_annotations_pushed_into_it() {
    let annotated = AnnotatedString::builder()
        .push_string_annotation("tag", "payload")
        .append("hello")
        .pop()
        .to_annotated_string();

    let found = annotated.get_string_annotations("tag", 0, annotated.text.len());
    assert_eq!(found.len(), 1, "the string annotation was not recorded");
    assert_eq!(found[0].item.annotation, "payload");

    assert!(
        annotated
            .get_string_annotations("other", 0, annotated.text.len())
            .is_empty()
    );
}

#[test]
fn an_annotated_string_carries_the_links_pushed_into_it() {
    let annotated = AnnotatedString::builder()
        .push_link(LinkAnnotation::Url("https://example.invalid".to_string()))
        .append("click me")
        .pop()
        .to_annotated_string();

    let links = annotated.get_link_annotations(0, annotated.text.len());
    assert_eq!(links.len(), 1, "the link was not recorded");
    assert_eq!(links[0].range.start, 0);
    assert_eq!(links[0].range.end, annotated.text.len());
}

#[test]
fn an_annotated_string_carries_paragraph_styles_over_the_range_they_cover() {
    let annotated = AnnotatedString::builder()
        .push_paragraph_style(ParagraphStyle::default())
        .append("a paragraph")
        .pop()
        .to_annotated_string();

    assert_eq!(
        annotated.paragraph_styles.len(),
        1,
        "the paragraph style was not recorded"
    );
    assert_eq!(annotated.paragraph_styles[0].range.start, 0);
    assert_eq!(
        annotated.paragraph_styles[0].range.end,
        annotated.text.len()
    );
}
