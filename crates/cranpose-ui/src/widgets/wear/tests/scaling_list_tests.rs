use super::*;

fn info(count: usize, viewport: f32, first: f32, last: f32) -> WearScalingLayoutInfo {
    WearScalingLayoutInfo {
        item_count: count,
        viewport,
        first_centre: first,
        last_centre: last,
        content: last - first,
        visible: count,
        composed: count,
    }
}

#[test]
fn a_transform_handle_is_the_cell_not_the_value() {
    let one = WearItemTransform::new();
    let shared = one.clone();
    let other = WearItemTransform::new();
    assert_eq!(one, shared);
    assert_ne!(one, other, "two fresh cells are two channels");
    shared.set(ScaleAlpha {
        scale: 0.7,
        alpha: 0.5,
    });
    assert_eq!(one.get().scale, 0.7, "a clone writes through");
}

#[test]
fn travel_is_measured_between_the_first_and_last_centres() {
    let info = info(3, 454.0, 227.0, 627.0);
    assert_eq!(info.travel(), 400.0);
    assert_eq!(info.scrolled(), 0.0, "at rest the first row is centred");
}

#[test]
fn scrolling_past_an_end_stops_instead_of_counting_on() {
    let info = info(3, 454.0, 227.0, 627.0);
    let anchor = CentreAnchor {
        index: 0,
        offset: 0.0,
    };
    let up = re_anchor(anchor, -50.0, info);
    assert_eq!(up.offset, 0.0, "already at the top");
    let down = re_anchor(anchor, 120.0, info);
    assert_eq!(down.offset, 120.0);
    let past = re_anchor(anchor, 900.0, info);
    assert_eq!(past.offset, 400.0, "clamped to the whole travel");
}

#[test]
fn an_empty_list_does_not_divide_by_its_own_travel() {
    let empty = WearScalingLayoutInfo::default();
    assert_eq!(empty.travel(), 0.0);
    assert_eq!(empty.scrolled(), 0.0);
    let anchor = CentreAnchor::default();
    assert_eq!(re_anchor(anchor, 30.0, empty).offset, 30.0);
}

#[test]
fn a_declared_row_carries_its_key_and_content_type() {
    let mut scope = WearScalingListScope::default();
    scope.item_keyed(Some(7), Some(1), || {});
    scope.items(
        LazyItems::new(3)
            .key(|index: usize| 100 + index as u64)
            .content_type(|index: usize| (index % 2) as u64),
        |_| {},
    );
    assert_eq!(scope.count(), 4);
    assert_eq!(scope.items[0].key, Some(7));
    assert_eq!(scope.items[0].content_type, Some(1));
    assert_eq!(
        scope.items[1..]
            .iter()
            .map(|item| item.key)
            .collect::<Vec<_>>(),
        [Some(100), Some(101), Some(102)]
    );
    assert_eq!(
        scope.items[1..]
            .iter()
            .map(|item| item.content_type)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(0)]
    );
}

#[test]
fn a_row_declared_without_a_key_is_identified_by_its_position() {
    let mut scope = WearScalingListScope::default();
    scope.items(2, |_| {});
    assert!(scope.items.iter().all(|item| item.key.is_none()));
    assert!(scope.items.iter().all(|item| item.content_type.is_none()));
}

#[test]
fn a_user_key_and_an_index_never_name_the_same_slot() {
    assert_ne!(
        LazyLayoutKey::User(3).to_slot_id(),
        LazyLayoutKey::Index(3).to_slot_id()
    );
}

fn summary_for(info: WearScalingLayoutInfo) -> (bool, bool) {
    let travel = info.travel();
    let scrolled = info.scrolled();
    (
        travel - scrolled > SCROLL_EPSILON,
        scrolled > SCROLL_EPSILON,
    )
}

#[test]
fn a_list_at_its_top_can_only_scroll_forward() {
    let (forward, backward) = summary_for(info(10, 200.0, 100.0, 500.0));
    assert!(forward);
    assert!(!backward);
}

#[test]
fn a_list_at_its_end_can_only_scroll_backward() {
    let (forward, backward) = summary_for(info(10, 200.0, -300.0, 100.0));
    assert!(!forward);
    assert!(backward);
}

#[test]
fn a_list_that_fits_can_scroll_neither_way() {
    let (forward, backward) = summary_for(info(1, 200.0, 100.0, 100.0));
    assert!(!forward);
    assert!(!backward);
}

#[test]
fn an_unmeasured_height_is_the_mean_of_the_measured_ones() {
    let mut heights = ItemHeights::default();
    heights.resize(4);
    heights.record(0, 30.0);
    heights.record(1, 50.0);
    assert_eq!(heights.height_of(0), 30.0);
    assert_eq!(heights.height_of(3), 40.0, "the mean of 30 and 50");
    assert_eq!(signed_span(&heights, 0, 2), 80.0);
    assert_eq!(signed_span(&heights, 2, 0), -80.0);
    assert_eq!(signed_span(&heights, 2, 2), 0.0);
}

#[test]
fn the_spec_defaults_are_the_ones_wear_ships() {
    let spec = WearScalingLazyColumnSpec::default();
    assert_eq!(spec.item_spacing, 4.0, "Arrangement.spacedBy(4.dp)");
    assert_eq!(
        spec.auto_centering,
        Some(CentreAnchor {
            index: 1,
            offset: 0.0
        }),
        "AutoCenteringParams(itemIndex = 1, itemOffset = 0)"
    );
    assert_eq!(spec.scaling, ScalingParams::WEAR);
}
