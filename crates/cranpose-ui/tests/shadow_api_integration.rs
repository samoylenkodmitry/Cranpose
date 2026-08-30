use cranpose_ui::{
    AppContext, BlendMode, Brush, Dp, DpOffset, DrawCommand, DrawCommandFn, LayerShape, Modifier,
    Point, RoundedCornerShape, Shadow, Size, collect_slices_from_modifier, command_draw_scope,
};
use cranpose_ui_graphics::{DrawPrimitive, DrawScope as _, ShadowPrimitive};

fn record(func: &DrawCommandFn, size: Size) -> Vec<DrawPrimitive> {
    let mut scope = command_draw_scope(size);
    func(&mut scope);
    scope.into_primitives()
}

fn contains_blend_mode(primitives: &[DrawPrimitive], mode: BlendMode) -> bool {
    primitives.iter().any(|primitive| match primitive {
        DrawPrimitive::Blend {
            primitive,
            blend_mode,
        } => *blend_mode == mode || contains_blend_mode(std::slice::from_ref(primitive), mode),
        DrawPrimitive::Shadow(ShadowPrimitive::Drop {
            shape, blend_mode, ..
        }) => *blend_mode == mode || contains_blend_mode(std::slice::from_ref(shape), mode),
        DrawPrimitive::Shadow(ShadowPrimitive::Inner {
            fill,
            cutout,
            blend_mode,
            ..
        }) => {
            *blend_mode == mode
                || mode == BlendMode::DstOut
                || contains_blend_mode(std::slice::from_ref(fill), mode)
                || contains_blend_mode(std::slice::from_ref(cutout), mode)
        }
        _ => false,
    })
}

fn contains_gradient_brush(primitives: &[DrawPrimitive]) -> bool {
    primitives.iter().any(|primitive| match primitive {
        DrawPrimitive::Rect { brush, .. } | DrawPrimitive::RoundRect { brush, .. } => {
            matches!(
                brush,
                Brush::LinearGradient { .. }
                    | Brush::RadialGradient { .. }
                    | Brush::SweepGradient { .. }
            )
        }
        DrawPrimitive::Blend { primitive, .. } => {
            contains_gradient_brush(std::slice::from_ref(primitive))
        }
        DrawPrimitive::Shadow(ShadowPrimitive::Drop { shape, .. }) => {
            contains_gradient_brush(std::slice::from_ref(shape))
        }
        DrawPrimitive::Shadow(ShadowPrimitive::Inner { fill, cutout, .. }) => {
            contains_gradient_brush(std::slice::from_ref(fill))
                || contains_gradient_brush(std::slice::from_ref(cutout))
        }
        _ => false,
    })
}

fn behind_command_tags(modifier: &Modifier) -> Vec<&'static str> {
    let slices = collect_slices_from_modifier(modifier);
    slices
        .draw_commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Behind(draw) => {
                let primitives = record(
                    draw,
                    Size {
                        width: 64.0,
                        height: 36.0,
                    },
                );
                let has_shadow = primitives
                    .iter()
                    .any(|primitive| matches!(primitive, DrawPrimitive::Shadow(_)));
                Some(if has_shadow { "shadow" } else { "paint" })
            }
            _ => None,
        })
        .collect()
}

#[test]
fn drop_and_inner_shadow_emit_expected_draw_layers() {
    let shape = LayerShape::Rounded(RoundedCornerShape::uniform(10.0));
    let modifier = Modifier::empty()
        .drop_shadow(shape, |scope| {
            scope.radius = 12.0;
            scope.spread = 3.0;
            scope.offset = Point::new(6.0, 8.0);
        })
        .background(cranpose_ui::Color::from_rgba_u8(200, 80, 70, 255))
        .rounded_corners(10.0)
        .inner_shadow(shape, |scope| {
            scope.radius = 10.0;
            scope.offset = Point::new(4.0, 3.0);
        });

    let slices = collect_slices_from_modifier(&modifier);
    assert_eq!(slices.draw_commands().len(), 3);

    let draw_size = Size {
        width: 72.0,
        height: 44.0,
    };
    let has_behind = slices
        .draw_commands()
        .iter()
        .any(|command| matches!(command, DrawCommand::Behind(_)));
    let has_overlay = slices
        .draw_commands()
        .iter()
        .any(|command| matches!(command, DrawCommand::Overlay(_)));
    assert!(has_behind, "drop shadow should render in behind phase");
    assert!(has_overlay, "inner shadow should render in overlay phase");

    let overlay_primitives = slices
        .draw_commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Overlay(draw) => Some(record(draw, draw_size)),
            _ => None,
        })
        .expect("overlay command expected");
    assert!(
        contains_blend_mode(&overlay_primitives, BlendMode::DstOut),
        "inner shadow overlay should carve interior using DstOut"
    );
}

#[test]
fn drop_shadow_before_background_renders_behind_paint() {
    let modifier = Modifier::empty()
        .drop_shadow(
            LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
            |scope| {
                scope.radius = 10.0;
                scope.offset = Point::new(4.0, 6.0);
            },
        )
        .background(cranpose_ui::Color::from_rgba_u8(220, 100, 80, 255))
        .rounded_corners(8.0);

    assert_eq!(
        behind_command_tags(&modifier),
        vec!["shadow", "paint"],
        "drop_shadow().background() must keep shadow behind content paint"
    );
}

#[test]
fn background_before_drop_shadow_renders_shadow_on_top() {
    let modifier = Modifier::empty()
        .background(cranpose_ui::Color::from_rgba_u8(220, 100, 80, 255))
        .rounded_corners(8.0)
        .drop_shadow(
            LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
            |scope| {
                scope.radius = 10.0;
                scope.offset = Point::new(4.0, 6.0);
            },
        );

    assert_eq!(
        behind_command_tags(&modifier),
        vec!["paint", "shadow"],
        "background().drop_shadow() should draw shadow above background due modifier order"
    );
}

#[test]
fn static_shadow_uses_density_when_converted_to_px() {
    let app_context = AppContext::new_with_density(2.0);
    app_context.enter(|| {
        let modifier = Modifier::empty().inner_shadow_value(
            LayerShape::Rectangle,
            Shadow {
                offset: DpOffset::new(Dp(2.5), Dp(0.0)),
                ..Default::default()
            },
        );
        let slices = collect_slices_from_modifier(&modifier);
        let overlay_primitives = slices
            .draw_commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Overlay(draw) => Some(record(
                    draw,
                    Size {
                        width: 40.0,
                        height: 20.0,
                    },
                )),
                _ => None,
            })
            .expect("overlay command expected");

        let cutout_x = overlay_primitives
            .iter()
            .find_map(|primitive| match primitive {
                DrawPrimitive::Shadow(ShadowPrimitive::Inner { cutout, .. }) => {
                    match cutout.as_ref() {
                        DrawPrimitive::Rect { rect, .. }
                        | DrawPrimitive::RoundRect { rect, .. } => Some(rect.x),
                        _ => None,
                    }
                }
                _ => None,
            });
        assert_eq!(cutout_x, Some(5.0));
    });
}

#[test]
fn shadow_brush_and_blend_mode_are_applied() {
    let modifier = Modifier::empty().drop_shadow(LayerShape::Rectangle, |scope| {
        scope.radius = 10.0;
        scope.spread = 3.0;
        scope.alpha = 0.8;
        scope.blend_mode = BlendMode::Overlay;
        scope.brush = Some(Brush::vertical_gradient(
            vec![
                cranpose_ui::Color::from_rgba_u8(220, 40, 180, 255),
                cranpose_ui::Color::from_rgba_u8(20, 160, 240, 220),
            ],
            0.0,
            40.0,
        ));
    });

    let slices = collect_slices_from_modifier(&modifier);
    let behind_primitives = slices
        .draw_commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Behind(draw) => Some(record(
                draw,
                Size {
                    width: 64.0,
                    height: 36.0,
                },
            )),
            _ => None,
        })
        .expect("behind command expected");

    assert!(
        contains_blend_mode(&behind_primitives, BlendMode::Overlay),
        "drop shadow should preserve configured blend mode"
    );
    assert!(
        contains_gradient_brush(&behind_primitives),
        "drop shadow should preserve configured gradient brush"
    );
}
