use super::*;

#[test]
fn shopping_bag_has_the_stacked_tab_icon_silhouette() {
    let bounds = VectorPath::parse(SHOPPING_BAG)
        .expect("shopping bag path")
        .bounds();
    assert!((bounds.width - 18.0).abs() < 1.0e-4);
    assert!((bounds.height - 19.0).abs() < 1.0e-4);
}
