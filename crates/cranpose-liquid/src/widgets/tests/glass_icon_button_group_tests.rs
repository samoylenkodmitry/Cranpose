use super::*;
use crate::widgets::GlassButtonStyle;

#[test]
fn icon_button_group_builders_and_hit_regions_preserve_member_gaps() {
    let spec = GlassIconButtonGroupSpec::new(44.0)
        .with_spacing(8.0)
        .with_pressed_scale(1.2)
        .with_glue_radius(12.0);
    assert_eq!(icon_group_width(2, spec), 96.0);
    assert_eq!(icon_group_item_at(22.0, 22.0, 2, spec), Some(0));
    assert_eq!(icon_group_item_at(48.0, 22.0, 2, spec), None);
    assert_eq!(icon_group_item_at(74.0, 22.0, 2, spec), Some(1));
    assert_eq!(icon_group_item_at(22.0, 50.0, 2, spec), None);

    let shapes = icon_group_neighbor_shapes(4, 2, spec, 16.0, 38.0);
    assert_eq!(shapes.len(), 2);
    assert!((shapes[0].0 - (16.0 + 52.0 + 22.0 + 44.0 * 0.38)).abs() < 1e-5);
    assert!((shapes[1].0 - (16.0 + 156.0 + 22.0 - 44.0 * 0.38)).abs() < 1e-5);
    assert_eq!(shapes[0].2, 44.0 * 0.36);

    let item = GlassIconButtonGroupItem::new("M0 0", "Confirm", || {})
        .with_spec(GlassButtonSpec::prominent());
    assert_eq!(item.content_description, "Confirm");
    assert_eq!(item.spec.style, GlassButtonStyle::Prominent);
}

#[test]
fn a_scope_declares_actions_in_order_with_their_callbacks() {
    let fired = Rc::new(Cell::new(0usize));
    let more = Rc::clone(&fired);
    let confirm = Rc::clone(&fired);
    let items = collect_items(|scope| {
        scope.action("M0 0", "More", move || more.set(1));
        scope.push(
            GlassIconButtonGroupItem::new("M1 1", "Confirm", move || confirm.set(2))
                .with_spec(GlassButtonSpec::prominent()),
        );
    });

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].content_description, "More");
    assert_eq!(items[0].spec.style, GlassButtonStyle::Glass);
    assert_eq!(items[1].spec.style, GlassButtonStyle::Prominent);

    (items[0].on_click.borrow_mut())();
    assert_eq!(fired.get(), 1);
    (items[1].on_click.borrow_mut())();
    assert_eq!(fired.get(), 2);
}

#[test]
fn a_group_with_no_actions_declares_none() {
    assert!(collect_items(|_| {}).is_empty());
}
