use super::*;

fn create_test_item(index: usize, offset: f32, size: f32) -> LazyListMeasuredItem {
    let mut item = LazyListMeasuredItem::new(index, index as u64, None, size, 100.0);
    item.offset = offset;
    item
}

#[test]
fn test_clamp_at_start() {
    let config = LazyListMeasureConfig::default();
    let adjuster = BoundsAdjuster::new(&config, 10, 500.0);

    let mut items = vec![
        create_test_item(0, 50.0, 100.0),
        create_test_item(1, 150.0, 100.0),
    ];

    adjuster.clamp_at_start(&mut items);

    assert_eq!(items[0].offset, 0.0);
    assert_eq!(items[1].offset, 100.0);
}

#[test]
fn test_clamp_at_start_with_content_padding() {
    let config = LazyListMeasureConfig {
        before_content_padding: 20.0,
        ..Default::default()
    };
    let adjuster = BoundsAdjuster::new(&config, 10, 500.0);

    let mut items = vec![
        create_test_item(0, 70.0, 100.0),
        create_test_item(1, 170.0, 100.0),
    ];

    adjuster.clamp_at_start(&mut items);

    assert_eq!(items[0].offset, 20.0);
    assert_eq!(items[1].offset, 120.0);
}

#[test]
fn test_clamp_at_end() {
    let config = LazyListMeasureConfig::default();
    let adjuster = BoundsAdjuster::new(&config, 5, 500.0);

    let mut items = vec![
        create_test_item(3, 100.0, 100.0),
        create_test_item(4, 200.0, 100.0),
    ];

    adjuster.clamp_at_end(&mut items);

    assert_eq!(items[1].offset + items[1].main_axis_size, 500.0);
}

#[test]
fn test_clamp_empty_items_is_noop() {
    let config = LazyListMeasureConfig::default();
    let adjuster = BoundsAdjuster::new(&config, 5, 500.0);
    let mut items = Vec::new();

    adjuster.clamp(&mut items);

    assert!(items.is_empty());
}

#[test]
fn test_no_clamp_when_not_at_bounds() {
    let config = LazyListMeasureConfig::default();
    let adjuster = BoundsAdjuster::new(&config, 100, 500.0);

    let mut items = vec![
        create_test_item(5, 0.0, 100.0),
        create_test_item(6, 100.0, 100.0),
    ];

    adjuster.clamp(&mut items);

    assert_eq!(items[0].offset, 0.0);
    assert_eq!(items[1].offset, 100.0);
}
