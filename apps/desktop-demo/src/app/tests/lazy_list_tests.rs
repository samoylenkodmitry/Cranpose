use super::{
    clamp_demo_item_count, DEFAULT_DEMO_ITEM_COUNT, MAX_DEMO_ITEM_COUNT, MIN_DEMO_ITEM_COUNT,
};

#[test]
fn clamp_demo_item_count_limits_values() {
    assert_eq!(clamp_demo_item_count(0), MIN_DEMO_ITEM_COUNT);
    assert_eq!(
        clamp_demo_item_count(DEFAULT_DEMO_ITEM_COUNT),
        DEFAULT_DEMO_ITEM_COUNT
    );
    assert_eq!(
        clamp_demo_item_count(MAX_DEMO_ITEM_COUNT + 1),
        MAX_DEMO_ITEM_COUNT
    );
}
