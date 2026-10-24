use cranpose_ui::{
    BoxWithConstraints, BoxWithConstraintsScope, Column, ColumnSpec, Modifier, Text,
    measure_layout, run_test_composition,
};
use cranpose_ui_graphics::Size;

#[test]
fn resizing_a_scoped_scroll_panel_keeps_layout_usable() {
    let mut composition = run_test_composition(|| {
        BoxWithConstraints(Modifier::empty().fill_max_size(), |scope| {
            let width = scope.constraints().max_width;
            if width < 900.0 {
                ScrollPanel(width);
            } else {
                Text(
                    "Wide workspace",
                    Modifier::empty(),
                    cranpose_ui::TextStyle::default(),
                );
            }
        });
    });
    let root = composition.root().expect("root");
    for width in [430.0, 1440.0, 1100.0, 768.0, 320.0, 430.0] {
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout =
            measure_layout(&mut applier, root, Size::new(width, 900.0)).expect("resized layout");
        assert_eq!(layout.root_size().width, width);
        applier.clear_runtime_handle();
    }
}

#[cranpose_macros::composable]
fn ScrollPanel(width: f32) {
    let scroll = cranpose_ui::rememberScrollState!(0.0);
    Column(
        Modifier::empty()
            .size_points(width, 300.0)
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        || {
            Text(
                "Scrollable content",
                Modifier::empty().height(900.0),
                cranpose_ui::TextStyle::default(),
            );
        },
    );
}
