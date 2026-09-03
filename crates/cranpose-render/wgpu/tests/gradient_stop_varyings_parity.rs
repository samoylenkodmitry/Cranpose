mod support;

use support::graph::*;

const SIZE: u32 = 224;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn record_gradient_scene(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        rect(0.0, 0.0, SIZE as f32, SIZE as f32),
        Brush::radial_gradient_stops(
            vec![
                (0.0, Color(0.09, 0.08, 0.18, 1.0)),
                (0.55, Color(0.04, 0.04, 0.10, 1.0)),
                (1.0, Color(0.02, 0.02, 0.04, 1.0)),
            ],
            Point::new(112.0, 22.0),
            212.0,
            TileMode::Clamp,
        ),
    );
    scope.draw_rect_at(
        rect(8.0, 8.0, 96.0, 40.0),
        Brush::linear_gradient_stops(
            vec![
                (0.2, Color(1.0, 0.2, 0.1, 1.0)),
                (0.9, Color(0.1, 0.3, 1.0, 1.0)),
            ],
            Point::new(0.0, 0.0),
            Point::new(96.0, 40.0),
            TileMode::Clamp,
        ),
    );
    scope.draw_rect_at(
        rect(120.0, 8.0, 96.0, 40.0),
        Brush::linear_gradient_stops(
            vec![
                (0.0, Color(0.9, 0.9, 0.2, 1.0)),
                (1.0, Color(0.2, 0.7, 0.3, 0.4)),
            ],
            Point::new(0.0, 0.0),
            Point::new(30.0, 0.0),
            TileMode::Repeated,
        ),
    );
    scope.draw_round_rect_at(
        rect(8.0, 60.0, 96.0, 60.0),
        Brush::radial_gradient_stops(
            vec![
                (0.0, Color(1.0, 1.0, 1.0, 1.0)),
                (0.35, Color(0.9, 0.4, 0.8, 0.9)),
                (0.7, Color(0.2, 0.1, 0.6, 1.0)),
            ],
            Point::new(48.0, 30.0),
            30.0,
            TileMode::Mirror,
        ),
        CornerRadii::uniform(12.0),
    );
    scope.draw_rect_at(
        rect(120.0, 60.0, 96.0, 60.0),
        Brush::radial_gradient_stops(
            vec![
                (0.1, Color(0.3, 1.0, 0.6, 1.0)),
                (0.5, Color(0.1, 0.5, 0.9, 1.0)),
                (0.8, Color(0.9, 0.2, 0.2, 1.0)),
                (1.0, Color(0.1, 0.1, 0.1, 0.0)),
            ],
            Point::new(48.0, 30.0),
            44.0,
            TileMode::Decal,
        ),
    );
    scope.draw_rect_at(
        rect(8.0, 132.0, 96.0, 40.0),
        Brush::sweep_gradient_stops(
            vec![
                (0.0, Color(1.0, 0.0, 0.0, 1.0)),
                (0.3, Color(0.0, 1.0, 0.0, 1.0)),
                (0.6, Color(0.0, 0.0, 1.0, 1.0)),
                (1.0, Color(1.0, 0.0, 0.0, 1.0)),
            ],
            Point::new(48.0, 20.0),
        ),
    );
    scope.draw_rect_at(
        rect(120.0, 132.0, 96.0, 40.0),
        Brush::linear_gradient_stops(
            vec![
                (0.0, Color(0.1, 0.1, 0.1, 1.0)),
                (0.2, Color(1.0, 0.5, 0.0, 1.0)),
                (0.4, Color(0.0, 0.8, 0.8, 1.0)),
                (0.7, Color(0.7, 0.0, 0.9, 1.0)),
                (1.0, Color(1.0, 1.0, 1.0, 1.0)),
            ],
            Point::new(0.0, 0.0),
            Point::new(96.0, 40.0),
            TileMode::Clamp,
        ),
    );
    scope.draw_rect_at(
        rect(8.0, 180.0, 40.0, 36.0),
        Brush::linear_gradient(vec![Color(0.4, 0.9, 0.3, 1.0)]),
    );
    scope.draw_rect_at_stroked(
        rect(56.0, 182.0, 48.0, 32.0),
        Brush::linear_gradient_stops(
            vec![
                (0.0, Color(1.0, 0.8, 0.2, 1.0)),
                (1.0, Color(0.2, 0.4, 1.0, 1.0)),
            ],
            Point::new(0.0, 0.0),
            Point::new(48.0, 32.0),
            TileMode::Clamp,
        ),
        Stroke {
            width: 4.0,
            ..Default::default()
        },
    );
    for i in 0..6u32 {
        let start = i as f32 * 1.0;
        scope.draw_annular_sector(
            Brush::radial_gradient_stops(
                vec![
                    (0.0, Color(1.0, 0.4, 0.2, 1.0)),
                    (0.5, Color(0.2, 0.9, 0.4, 1.0)),
                    (1.0, Color(0.3, 0.3, 1.0, 1.0)),
                ],
                Point::new(22.0, 22.0),
                24.0,
                TileMode::Clamp,
            ),
            Point::new(168.0, 198.0),
            12.0,
            22.0,
            start,
            0.8,
        );
    }
}

fn gradient_graph() -> RenderGraph {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    record_gradient_scene(&mut scope);
    draw_run_graph(SIZE, scope)
}

fn render_arm(uniform_stops: bool) -> Option<Vec<u8>> {
    cranpose_render_wgpu::set_debug_toggle(
        "CRANPOSE_UNIFORM_GRADIENT_STOPS",
        uniform_stops.then_some("1"),
    );
    let pixels = render_with_current_toggles();
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_UNIFORM_GRADIENT_STOPS", None);
    let pixels = pixels?;
    let device_errors = pixels.1;
    assert_eq!(
        device_errors, 0,
        "uniform_stops={uniform_stops}: the device recorded a validation error, so the \
         frame is whatever the failed pipeline left behind"
    );
    Some(pixels.0)
}

fn render_with_current_toggles() -> Option<(Vec<u8>, u64)> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping gradient stop parity: headless WGPU init failed: {err}");
            return None;
        }
    };
    let pixels = support::stable_capture(&mut renderer, &gradient_graph(), SIZE);
    Some((pixels, renderer.device_error_count_for_tests()))
}

#[test]
fn inline_gradient_stops_match_the_uniform_stop_walk_byte_for_byte() {
    for instanced in ["0", "1"] {
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", Some(instanced));
        let arms = (render_arm(false), render_arm(true));
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_INSTANCED_QUADS", None);
        let (Some(inline), Some(uniform)) = arms else {
            return;
        };
        let distinct = support::distinct_colors(&inline);
        if let Ok(dir) = std::env::var("CRANPOSE_PARITY_DUMP_DIR") {
            let rgb: Vec<u8> = inline
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|px| [px[0], px[1], px[2]])
                .collect();
            std::fs::write(
                format!("{dir}/gradient_parity_{instanced}.ppm"),
                [format!("P6 {SIZE} {SIZE} 255\n").as_bytes(), &rgb].concat(),
            )
            .unwrap();
        }
        assert!(
            distinct > 2_000,
            "instanced={instanced}: {distinct} distinct colors — the scene must be a ramp, \
             not a flat fill"
        );
        support::assert_same_bytes(
            &format!(
                "instanced={instanced}: inline stop varyings vs the uniform stop walk; the inline \
                 path runs the same segment arithmetic over the same stop values"
            ),
            SIZE,
            &inline,
            &uniform,
        );
    }
}
