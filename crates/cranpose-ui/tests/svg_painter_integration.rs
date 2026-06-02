use cranpose_ui::{
    Alignment, Color, ColorFilter, ContentScale, HeadlessRenderer, Image, LayoutEngine, Modifier,
    PaintLayer, RenderOp, Size, SvgPainter, DEFAULT_ALPHA,
};

const ICON_SVG: &[u8] = br##"
    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
      <path d="M4 12h16" fill="none" stroke="#000000" stroke-width="4" stroke-linecap="round"/>
      <path d="M12 4v16" fill="none" stroke="#000000" stroke-width="4" stroke-linecap="round"/>
    </svg>
"##;

#[test]
fn svg_painter_renders_through_image_composable() {
    let painter = SvgPainter::from_bytes(ICON_SVG).expect("svg painter");
    let mut composition = cranpose_ui::run_test_composition({
        let painter = painter.clone();
        move || {
            Image(
                painter.clone(),
                Some("Add".to_string()),
                Modifier::empty().size(Size::new(24.0, 24.0)),
                Alignment::CENTER,
                ContentScale::Fit,
                DEFAULT_ALPHA,
                Some(ColorFilter::tint(Color::from_rgba_u8(10, 20, 30, 255))),
            );
        }
    });

    let root = composition.root().expect("image root");
    let handle = composition.runtime_handle();
    let layout = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(root, Size::new(64.0, 64.0))
            .expect("layout");
        applier.clear_runtime_handle();
        layout
    };

    let scene = composition.with_app_context(|| HeadlessRenderer::new().render(&layout));
    assert!(scene.operations().iter().any(|op| {
        matches!(
            op,
            RenderOp::Primitive {
                layer: PaintLayer::Behind,
                ..
            }
        )
    }));
}
