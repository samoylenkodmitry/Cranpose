mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{IsolationReasons, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect,
    RenderEffect, RuntimeShader,
};

fn effect(mask: bool, selected: bool, dark: bool) -> RenderEffect {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}\n{}",
        cranpose_ui_graphics::LIQUID_GLASS_GEOMETRY_WGSL,
        include_str!("../../../cranpose-liquid/src/widgets/vibrancy.wgsl")
    ));
    shader.set_override("VIBRANCY_MASK", f64::from(mask));
    shader.set_float4(0, 0.9, 64.0, 48.0, 0.0);
    shader.set_float4(4, 22.0, 22.0, if selected { 12.0 } else { 0.0 }, 20.0);
    shader.set_float4(8, 0.0, if dark { 145.0 } else { 136.0 } / 255.0, 1.0, 1.0);
    shader.set_float(12, f32::from(dark));
    shader.set_batched_source(!mask);
    shader.set_substrates(if mask {
        &[]
    } else {
        &[cranpose_ui_graphics::SubstrateSpec::Mean]
    });
    RenderEffect::runtime_shader(shader)
}

fn scene(selected: bool, dark: bool) -> RenderGraph {
    let background = if dark {
        Color(0.2, 0.32, 0.1, 1.0)
    } else {
        Color(0.7, 0.96, 0.83, 1.0)
    };
    source_scene(selected, dark, background)
}

fn source_scene(selected: bool, dark: bool, background: Color) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 48.0,
    };
    let mut mask = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            blend_mode: BlendMode::DstOut,
            render_effect: Some(effect(true, selected, dark)),
            ..Default::default()
        },
        vec![support::solid_rect(
            Rect {
                x: 16.0,
                y: 12.0,
                width: 24.0,
                height: 20.0,
            },
            Color::BLACK,
        )],
    );
    mask.isolation = IsolationReasons {
        effect: true,
        blend_mode: true,
        ..Default::default()
    };
    let mut backdrop = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            backdrop_effect: Some(effect(false, selected, dark)),
            ..Default::default()
        },
        vec![],
    );
    backdrop.isolation = IsolationReasons {
        backdrop: true,
        ..Default::default()
    };
    let mut ink = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        },
        vec![
            RenderNode::Layer(Box::new(backdrop)),
            RenderNode::Layer(Box::new(mask)),
        ],
    );
    ink.isolation = IsolationReasons {
        explicit_offscreen: true,
        ..Default::default()
    };
    RenderGraph::new(shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            support::solid_rect(bounds, background),
            RenderNode::Layer(Box::new(ink)),
        ],
    ))
}

#[test]
fn vibrant_ink_uses_the_backdrop_only_within_the_content_mask() {
    let mut renderer = support::headless_renderer().expect("GPU vibrancy test must run");
    for selected in [false, true] {
        let graph = scene(selected, false);
        renderer.scene_mut().graph = Some(graph);
        let frame = renderer.capture_frame(64, 48).expect("vibrancy capture");
        for (x, y, expected) in [
            (4, 4, [179, 245, 212, 255]),
            (
                24,
                20,
                if selected {
                    [0, 131, 234, 255]
                } else {
                    [0, 39, 6, 255]
                },
            ),
            (50, 40, [179, 245, 212, 255]),
        ] {
            let offset = (y * 64 + x) * 4;
            for (actual, expected) in frame.pixels[offset..offset + 4].iter().zip(expected) {
                assert!(
                    (i32::from(*actual) - expected).abs() <= 1,
                    "vibrancy at {x},{y}: {:?}",
                    &frame.pixels[offset..offset + 4]
                );
            }
        }
    }
}

fn configure_native_content_shader(shader: &mut RuntimeShader) {
    shader.set_float4(0, 0.9, 402.0, 120.0, 1.16);
    shader.set_float4(4, 334.54645, 61.0, 115.3592, 72.74904);
    shader.set_float4(8, 0.0, 136.0 / 255.0, 1.0, 1.0);
    shader.set_float4(12, 0.0, 1.0, 115.3592 / 111.0, 0.0);
    shader.set_float4(20, 334.54645, 61.0, 1.0, 1.0);
}

fn content_test_frame(
    renderer: &mut cranpose_render_wgpu::WgpuRenderer,
    shader: RuntimeShader,
    brush: cranpose_ui_graphics::Brush,
) -> cranpose_render_wgpu::CapturedFrame {
    let bounds = Rect::from_size(cranpose_ui_graphics::Size::new(1206.0, 360.0));
    let layer = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(shader)),
            ..Default::default()
        },
        vec![support::brush_rect(bounds, brush)],
    );
    let root = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            support::solid_rect(bounds, Color::WHITE),
            RenderNode::Layer(Box::new(layer)),
        ],
    );
    renderer.scene_mut().graph = Some(RenderGraph::new(root));
    renderer.capture_frame(1206, 360).unwrap()
}

#[test]
fn moving_lens_preserves_every_central_content_pixel() {
    let mut renderer = support::headless_renderer().expect("GPU content anchor test must run");
    let RenderEffect::Shader { shader } = effect(true, true, false) else {
        panic!("ink shader")
    };
    let mut shader = (*shader).clone();
    configure_native_content_shader(&mut shader);
    let brush = cranpose_ui_graphics::Brush::linear_gradient_range(
        vec![Color::TRANSPARENT, Color::WHITE],
        cranpose_ui_graphics::Point::ZERO,
        cranpose_ui_graphics::Point::new(1206.0, 0.0),
    );
    let frames = [-14.0, 14.0, -14.0].map(|offset| {
        shader.set_float4(4, 334.54645 + offset, 61.0, 115.3592, 72.74904);
        content_test_frame(&mut renderer, shader.clone(), brush.clone())
    });
    let mut checked = 0;
    for frame in &frames[1..] {
        for y in 175..195 {
            for x in 990..1010 {
                let pixel = (y * 1206 + x) * 4;
                assert_eq!(
                    &frames[0].pixels[pixel..pixel + 4],
                    &frame.pixels[pixel..pixel + 4],
                    "moving the lens changed anchored content at {x},{y}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 800);
}

#[test]
fn selected_glyph_mask_samples_the_native_refracted_content_coordinates() {
    let mut renderer = support::headless_renderer().expect("GPU content mask must run");
    let RenderEffect::Shader { shader } = effect(true, true, false) else {
        panic!("ink shader")
    };
    let mut shader = (*shader).clone();
    configure_native_content_shader(&mut shader);
    let mut count = 0;
    for projection in [(1.0, 1.0), (1.1, 0.8)] {
        shader.set_float2(16, projection.0, projection.1);
        shader.set_float4(
            4,
            334.54645,
            61.0,
            115.3592 * projection.0,
            72.74904 * projection.1,
        );
        for axis in 0..2 {
            let extent = if axis == 0 { 1206.0 } else { 360.0 };
            let center = if axis == 0 { 334.54645 } else { 61.0 };
            let brush = cranpose_ui_graphics::Brush::linear_gradient_range(
                vec![Color::TRANSPARENT, Color::WHITE],
                cranpose_ui_graphics::Point::ZERO,
                cranpose_ui_graphics::Point::new(
                    if axis == 0 { extent } else { 0.0 },
                    if axis == 1 { extent } else { 0.0 },
                ),
            );
            let image = content_test_frame(&mut renderer, shader.clone(), brush);
            for row in include_str!("fixtures/native_content_kernel.csv")
                .lines()
                .skip(1)
            {
                let values = row
                    .split(',')
                    .map(|v| v.parse::<f32>().unwrap())
                    .collect::<Vec<_>>();
                let warped = (values[axis] + 0.5) / 3.0 + values[axis + 2];
                let expected = (center
                    + (warped - center) * [projection.0, projection.1][axis] / 1.16)
                    / (extent / 3.0)
                    * 255.0;
                let x = (334.54645 * 3.0 + (values[0] + 0.5 - 334.54645 * 3.0) * projection.0 - 0.5)
                    .round() as usize;
                let y = (183.0 + (values[1] + 0.5 - 183.0) * projection.1 - 0.5).round() as usize;
                let actual = image.pixels[(y * 1206 + x) * 4];
                assert!(
                    (f32::from(actual) - expected).abs() <= 3.0,
                    "content mask at {},{} axis {axis}: native {expected}, Cranpose {actual}",
                    values[0],
                    values[1]
                );
                count += 1;
            }
        }
    }
    assert_eq!(count, 128);
}

#[test]
fn content_refraction_matches_the_native_displacement_map() {
    let mut renderer = support::headless_renderer().expect("GPU content warp must run");
    let source = include_str!("../../../cranpose-liquid/src/widgets/vibrancy.wgsl")
        .replace("fn effect_fs", "fn vibrancy_fs");
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}\n{source}\n\
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{\n\
             let field = content_displacement(input.uv * vec2<f32>(402.0, 120.0), 3.0) / (-17.5 * u[3].z);\n\
             return vec4<f32>(vec2<f32>(0.5) + field * 0.25, 0.75, 1.0);\n\
         }}",
        cranpose_ui_graphics::LIQUID_GLASS_GEOMETRY_WGSL,
    ));
    configure_native_content_shader(&mut shader);
    let image = content_test_frame(
        &mut renderer,
        shader,
        cranpose_ui_graphics::Brush::Solid(Color::BLACK),
    );
    let mut count = 0;
    for row in include_str!("fixtures/native_content_displacement.csv")
        .lines()
        .skip(1)
    {
        let values = row
            .split(',')
            .map(|v| v.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        for channel in 0..2 {
            let actual = image.pixels[(values[1] * 1206 + values[0]) * 4 + channel];
            assert!(
                (i32::from(actual) - values[channel + 2] as i32).abs() <= 3,
                "native content field at {},{} channel {channel}: native {}, Cranpose {actual}",
                values[0],
                values[1],
                values[channel + 2]
            );
        }
        count += 1;
    }
    assert_eq!(count, 32);
}

#[test]
fn optical_projection_preserves_the_native_capsule_and_unmagnified_source() {
    use cranpose_liquid::prelude::*;
    let mut renderer = support::headless_renderer().expect("GPU optical projection must run");
    let bounds = Rect::from_size(cranpose_ui_graphics::Size::new(402.0, 120.0));
    let color = cranpose_liquid::LiquidColors::light(Color::BLUE);
    let projection = (1.2, 0.75);
    let glass = Glass::clear().no_clip();
    let dynamics = GlassDynamics {
        optical_projection: Some(projection),
        morph: Some(GlassMorph {
            node_size: (402.0, 120.0),
            primary: (201.0, 61.0, 111.0, 70.0, 35.0),
            ..Default::default()
        }),
        ..Default::default()
    };
    let effect = glass.backdrop_effect(&color, 3.0, dynamics);
    let mut shader = terminal_effect_shader(&effect).unwrap().clone();
    shader.set_float(112, 1.0);
    shader.set_float(135, 1.0);
    cranpose_ui_graphics::specialize_liquid_glass(&mut shader);
    let layer = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(shader)),
            ..Default::default()
        },
        vec![support::brush_rect(
            bounds,
            cranpose_ui_graphics::Brush::horizontal_gradient(
                vec![Color(0.0, 1.0, 0.0, 1.0), Color(1.0, 1.0, 0.0, 1.0)],
                0.0,
                402.0,
            ),
        )],
    );
    renderer.scene_mut().graph = Some(RenderGraph::new(shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::uniform_scale(3.0),
        GraphicsLayer::default(),
        vec![
            support::solid_rect(bounds, Color::BLACK),
            RenderNode::Layer(Box::new(layer)),
        ],
    )));
    let frame = renderer.capture_frame(1206, 360).unwrap();
    let mut tested = 0;
    for (local_x, local_y, inside) in [
        (-48.0, -19.0, true),
        (-48.0, 19.0, true),
        (-50.0, -23.0, false),
        (-50.0, 23.0, false),
        (48.0, -19.0, true),
        (48.0, 19.0, true),
    ] {
        let x = ((201.0 + local_x * projection.0) * 3.0) as usize;
        let y = ((61.0 + local_y * projection.1) * 3.0) as usize;
        let offset = (y * 1206 + x) * 4;
        let pixel = &frame.pixels[offset..offset + 4];
        if inside {
            assert_eq!(pixel[1], 255, "projected native cap at {x},{y}: {pixel:?}");
            let expected = (x as f32 + 0.5) / 1206.0 * 255.0;
            assert!(
                (f32::from(pixel[0]) - expected).abs() <= 1.0,
                "backdrop coordinate at {x},{y}: {pixel:?}"
            );
        } else {
            assert_eq!(pixel[0], 0, "outside native cap at {x},{y}");
        }
        tested += 1;
    }
    assert_eq!(tested, 6);
}

#[test]
fn dark_vibrancy_matches_native_color_matrices() {
    let mut renderer = support::headless_renderer().expect("dark vibrancy requires GPU");
    for selected in [false, true] {
        renderer.scene_mut().graph = Some(scene(selected, true));
        let frame = renderer
            .capture_frame(64, 48)
            .expect("dark vibrancy capture");
        let rows = include_str!("fixtures/native_ink_matrices.csv")
            .lines()
            .skip(1)
            .map(|line| {
                line.split(',')
                    .map(|value| value.parse::<f32>().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let matrix = rows
            .iter()
            .find(|row| row[0] == 1.0 && row[1] == f32::from(selected))
            .expect("native dark matrix");
        for channel in 0..3 {
            let coefficients = &matrix[2 + channel * 4..];
            let expected = (coefficients[0] * 51.0
                + coefficients[1] * 82.0
                + coefficients[2] * 26.0
                + coefficients[3] * 255.0)
                .clamp(0.0, 255.0);
            let actual = frame.pixels[(20 * 64 + 24) * 4 + channel];
            assert!(
                (f32::from(actual) - expected).abs() <= 1.0,
                "dark ink selected={selected}, channel {channel}: native {expected}, Cranpose {actual}"
            );
        }
    }
}

#[test]
fn ink_polarity_follows_the_glass_when_its_tone_reverses() {
    let mut renderer = support::headless_renderer().expect("adaptive ink requires GPU");
    for (foreground_is_light, background, expected) in
        [(true, Color::WHITE, 26), (false, Color::BLACK, 242)]
    {
        renderer.scene_mut().graph = Some(source_scene(false, foreground_is_light, background));
        let frame = renderer
            .capture_frame(64, 48)
            .expect("adaptive ink capture");
        for channel in 0..3 {
            let actual = frame.pixels[(20 * 64 + 24) * 4 + channel];
            assert!(
                actual.abs_diff(expected) <= 1,
                "ink polarity on {background:?}: expected {expected}, actual {actual}"
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TabBackdrop {
    Green,
    Foreground {
        accent: Color,
        icon: usize,
    },
    Flat(f32),
    DarkFlat(f32),
    Wave {
        horizontal: bool,
        period: f32,
        phase: f32,
        channel: Option<usize>,
    },
}

impl TabBackdrop {
    fn color(self) -> Color {
        match self {
            Self::Foreground { .. } => Color::WHITE,
            Self::Flat(level) | Self::DarkFlat(level) => Color(level, level, level, 1.0),
            _ => Color(0.7, 0.96, 0.83, 1.0),
        }
    }
}

fn wave_color(level: f32, channel: Option<usize>) -> Color {
    let [red, green, blue] = std::array::from_fn(|index| {
        if channel.is_none_or(|channel| channel == index) {
            level
        } else {
            0.5
        }
    });
    Color(red, green, blue, 1.0)
}

#[cranpose_ui::composable]
#[allow(non_snake_case)]
fn VibrantTabScene(backdrop: TabBackdrop) {
    let selected =
        cranpose_core::remember(|| cranpose_core::mutableStateOf(0usize)).with(|state| *state);
    use cranpose_liquid::prelude::*;
    use cranpose_ui::{
        Modifier, Size,
        widgets::{Box, BoxSpec},
    };
    LiquidTheme(
        LiquidThemeSpec {
            accent: match backdrop {
                TabBackdrop::Foreground { accent, .. } => accent,
                _ => LiquidThemeSpec::default().accent,
            },
            scheme: if matches!(backdrop, TabBackdrop::DarkFlat(_)) {
                SchemeMode::Dark
            } else {
                SchemeMode::Light
            },
            ..LiquidThemeSpec::default()
        },
        move || {
            Box(
                Modifier::empty()
                    .size(Size::new(402.0, 120.0))
                    .background(backdrop.color())
                    .draw_behind(move |scope| {
                        if let TabBackdrop::Wave {
                            horizontal,
                            period,
                            phase,
                            channel,
                        } = backdrop
                        {
                            let extent = if horizontal { 1206 } else { 360 };
                            for pixel in 0..extent {
                                let position = (pixel as f32 + 0.5) / 3.0
                                    + if horizontal { 0.0 } else { 761.0 };
                                let level = 0.5
                                    + 0.15
                                        * (std::f32::consts::TAU * position / period + phase).cos();
                                scope.draw_rect_at(
                                    cranpose_ui::Rect {
                                        x: if horizontal { pixel as f32 / 3.0 } else { 0.0 },
                                        y: if horizontal { 0.0 } else { pixel as f32 / 3.0 },
                                        width: if horizontal { 1.0 / 3.0 } else { 402.0 },
                                        height: if horizontal { 120.0 } else { 1.0 / 3.0 },
                                    },
                                    cranpose_ui::Brush::solid(wave_color(level, channel)),
                                );
                            }
                        }
                    }),
                BoxSpec::default(),
                move || {
                    LiquidTabBar(
                        Modifier::empty().offset(21.0, 30.0).width(360.0),
                        LiquidTabBarSpec::new(88.0),
                        selected.get(),
                        move |index| selected.set(index),
                        move |tabs| reference_tabs(tabs, backdrop),
                    );
                },
            );
        },
    );
}

fn reference_tabs(tabs: &cranpose_liquid::prelude::LiquidTabBarScope, backdrop: TabBackdrop) {
    use cranpose_liquid::prelude::LiquidTab;
    use cranpose_ui::Size;
    let bytes = match backdrop {
        TabBackdrop::Foreground { icon: 0, .. } => {
            include_bytes!("../../../../apps/liquid-reference/reference-content/saved.png")
                .as_slice()
        }
        TabBackdrop::Foreground { icon: 1, .. } => {
            include_bytes!("../../../../apps/liquid-reference/reference-content/browse.png")
                .as_slice()
        }
        _ => include_bytes!("../../../../apps/liquid-reference/reference-content/account.png")
            .as_slice(),
    };
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("native PNG header");
    let mut pixels = vec![0; reader.output_buffer_size().expect("native PNG size")];
    let info = reader.next_frame(&mut pixels).expect("native PNG pixels");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    if !matches!(
        backdrop,
        TabBackdrop::Green | TabBackdrop::Foreground { .. }
    ) {
        pixels.fill(0);
    }
    let size = Size::new(info.width as f32 / 3.0, info.height as f32 / 3.0);
    let bitmap = cranpose_ui_graphics::ImageBitmap::from_rgba8(info.width, info.height, pixels)
        .expect("native bitmap");
    for label in ["Discover", "Browse", "Saved", "Account"] {
        let tab = LiquidTab::from_painter(
            cranpose_ui::widgets::Painter::from_bitmap(bitmap.clone()),
            size,
            if !matches!(
                backdrop,
                TabBackdrop::Green | TabBackdrop::Foreground { .. }
            ) {
                ""
            } else {
                label
            },
        );
        tabs.push(if matches!(backdrop, TabBackdrop::Foreground { .. }) {
            tab.with_icon_offset(0.0, -5.0)
        } else {
            tab
        });
    }
}

fn with_tab_shell<T>(
    backdrop: TabBackdrop,
    run: impl FnOnce(&mut cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>) -> T,
) -> T {
    let (_lock, renderer) =
        support::headless_renderer_parts().expect("GPU tab composition test must run");
    let mut shell = cranpose_app_shell::AppShell::new(
        renderer,
        cranpose_core::location_key(file!(), line!(), column!()),
        move || VibrantTabScene(backdrop),
    );
    cranpose_ui_graphics::set_glass_material_folds(false);
    shell.renderer().set_root_scale(3.0);
    shell.set_density(3.0);
    shell.set_viewport(402.0, 120.0);
    shell.set_buffer_size(1206, 360);
    for _ in 0..3 {
        shell.update();
    }
    run(&mut shell)
}

fn held_tab_pixels(backdrop: TabBackdrop) -> Vec<u8> {
    with_tab_shell(backdrop, |shell| held_tab_capture(shell, backdrop))
}

fn held_tab_capture(
    shell: &mut cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>,
    backdrop: TabBackdrop,
) -> Vec<u8> {
    held_tab_capture_with_input(shell, backdrop, false)
}

fn held_tab_capture_with_input(
    shell: &mut cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>,
    backdrop: TabBackdrop,
    native_input: bool,
) -> Vec<u8> {
    assert!(shell.set_cursor(330.0, 61.0));
    assert!(shell.pointer_pressed());
    for _ in 0..240 {
        shell.update_after_exact_interval(std::time::Duration::from_millis(16));
    }
    if native_input {
        replace_pane_input(shell.renderer(), backdrop);
    }
    let frame = shell
        .renderer()
        .capture_frame(1206, 360)
        .expect("composed tab capture");
    if let Ok(root) = std::env::var("CRANPOSE_OPTICAL_DEBUG") {
        let name = match backdrop {
            TabBackdrop::Foreground { accent, icon } => format!("foreground-{accent:?}-{icon}"),
            TabBackdrop::Green => "green".to_string(),
            TabBackdrop::Flat(level) => format!("flat-{level}"),
            TabBackdrop::DarkFlat(level) => format!("dark-flat-{level}"),
            TabBackdrop::Wave {
                horizontal,
                period,
                phase,
                channel,
            } => format!(
                "{}-{period}-{}-{channel:?}",
                if horizontal { "x" } else { "y" },
                (phase / std::f32::consts::FRAC_PI_2).round()
            ),
        };
        let prefix = if native_input { "native-input-" } else { "" };
        save_optical_capture(&root, &format!("{prefix}{name}"), &frame);
    }
    frame.pixels
}

fn save_optical_capture(root: &str, name: &str, frame: &cranpose_render_wgpu::CapturedFrame) {
    let path = std::path::Path::new(root).join(format!("{name}.png"));
    let mut encoder = png::Encoder::new(std::fs::File::create(&path).unwrap(), 1206, 360);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&frame.pixels)
        .unwrap();
}

fn replace_pane_input(renderer: &mut cranpose_render_wgpu::WgpuRenderer, backdrop: TabBackdrop) {
    use cranpose_render_common::graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
    };
    use cranpose_ui_graphics::{DrawPrimitive, ImageBitmap, ImageSampling};
    let index = match backdrop {
        TabBackdrop::Flat(level) => usize::from(level > 0.5),
        TabBackdrop::Wave {
            horizontal,
            period,
            phase,
            channel,
        } => {
            assert_eq!(channel, None);
            assert_eq!(period, 32.0);
            2 + usize::from(!horizontal) * 4
                + (phase / std::f32::consts::FRAC_PI_2).round() as usize
        }
        TabBackdrop::Green | TabBackdrop::DarkFlat(_) | TabBackdrop::Foreground { .. } => {
            panic!("pane input requires a light optical probe")
        }
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/native_pane/probe-{index:02}.png"));
    let mut decoder =
        png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    decoder.set_transformations(png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    let image = ImageBitmap::from_rgba8(info.width, 339, pixels).unwrap();
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 402.0,
        height: 120.0,
    };
    let primitive = RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Image {
                rect: Rect {
                    height: 113.0,
                    ..bounds
                },
                image,
                alpha: 1.0,
                color_filter: None,
                sampling: ImageSampling::Nearest,
                src_rect: None,
            },
            clip: None,
        }),
    });
    let lens = extract_lens(
        &renderer.scene_mut().graph.as_ref().unwrap().root,
        ProjectiveTransform::identity(),
    )
    .expect("composed raised lens");
    renderer.scene_mut().graph = Some(RenderGraph::new(shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![primitive, RenderNode::Layer(Box::new(lens))],
    )));
}

fn extract_lens(
    layer: &cranpose_render_common::graph::LayerNode,
    parent: ProjectiveTransform,
) -> Option<cranpose_render_common::graph::LayerNode> {
    let mut foreground = extract_lens_stage(layer, parent, 3.0)?;
    let background = extract_lens_stage(layer, parent, 2.0)?;
    assert_eq!(
        foreground.transform_to_parent,
        background.transform_to_parent
    );
    foreground.graphics_layer.backdrop_effect = Some(
        background
            .graphics_layer
            .backdrop_effect
            .unwrap()
            .then(foreground.graphics_layer.backdrop_effect.take().unwrap()),
    );
    Some(foreground)
}

fn extract_lens_stage(
    layer: &cranpose_render_common::graph::LayerNode,
    parent: ProjectiveTransform,
    stage: f32,
) -> Option<cranpose_render_common::graph::LayerNode> {
    let transform = layer.transform_to_parent.then(parent);
    if layer
        .graphics_layer
        .backdrop_effect
        .as_ref()
        .and_then(terminal_effect_shader)
        .is_some_and(|shader| {
            shader.uniforms().len() > 147
                && shader.uniforms()[130] == 2.0
                && shader.uniforms()[147] == stage
        })
    {
        let mut lens = layer.clone();
        lens.transform_to_parent = transform;
        return Some(lens);
    }
    layer.children.iter().find_map(|child| match child {
        RenderNode::Layer(child) => extract_lens_stage(child, transform, stage),
        _ => None,
    })
}

fn terminal_effect_shader(effect: &RenderEffect) -> Option<&RuntimeShader> {
    match effect {
        RenderEffect::Shader { shader } => Some(shader),
        RenderEffect::Chain { second, .. } => terminal_effect_shader(second),
        _ => None,
    }
}

fn rendered_lens_bounds(
    layer: &cranpose_render_common::graph::LayerNode,
    parent: ProjectiveTransform,
) -> Option<Rect> {
    rendered_glass_bounds(layer, parent, 2.0)
}

fn rendered_glass_bounds(
    layer: &cranpose_render_common::graph::LayerNode,
    parent: ProjectiveTransform,
    mode: f32,
) -> Option<Rect> {
    let transform = layer.transform_to_parent.then(parent);
    if let Some(shader) = layer
        .graphics_layer
        .backdrop_effect
        .as_ref()
        .and_then(terminal_effect_shader)
    {
        let u = shader.uniforms();
        if u.len() > 130 && u[130] == mode && mode == 1.0 {
            return Some(transform.bounds_for_rect(layer.local_bounds));
        }
        if u.len() > 147 && u[130] == mode && u[147] == 3.0 {
            let projection = if u.len() > 169 && u[168] > 0.0 && u[169] > 0.0 {
                (u[168], u[169])
            } else {
                (1.0, 1.0)
            };
            return Some(transform.bounds_for_rect(Rect {
                x: u[2] - u[4] * projection.0 * 0.5,
                y: u[3] - u[5] * projection.1 * 0.5,
                width: u[4] * projection.0,
                height: u[5] * projection.1,
            }));
        }
    }
    layer.children.iter().find_map(|child| match child {
        RenderNode::Layer(child) => rendered_glass_bounds(child, transform, mode),
        _ => None,
    })
}

#[test]
fn composed_contact_keeps_refraction_and_shape_at_the_same_native_progress() {
    with_tab_shell(TabBackdrop::Flat(0.35), |shell| {
        assert!(shell.set_cursor(72.333, 61.0));
        assert!(shell.pointer_pressed_at_event_time(shell.exact_pointer_event_time(None)));
        for frame in 1..=15 {
            shell.update_after_exact_interval(std::time::Duration::from_nanos(16_666_667));
            let lens = extract_lens(
                &shell.renderer().scene_mut().graph.as_ref().unwrap().root,
                ProjectiveTransform::identity(),
            )
            .unwrap();
            let shader =
                terminal_effect_shader(lens.graphics_layer.backdrop_effect.as_ref().unwrap())
                    .unwrap();
            let u = shader.uniforms();
            let shape_progress = ((u[4] - 95.0) / 16.0).clamp(0.0, 1.0);
            let refraction_progress =
                u[cranpose_ui_graphics::GLASS_EDGE_REFRACTION_REACH_UNIFORM] / 9.0;
            assert_eq!(
                u[cranpose_ui_graphics::GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM],
                36.0,
                "native outer warp keeps its 36-point profile while its strength animates"
            );
            assert!(
                (u[cranpose_ui_graphics::GLASS_EDGE_RETURN_DEPTH_UNIFORM] - 7.0 * shape_progress)
                    .abs()
                    <= 0.001,
                "native inner return depth follows contact independently of the outer profile"
            );
            assert!(
                (shape_progress - refraction_progress).abs() <= 0.001,
                "frame {frame}: native shape and refraction share progress, Cranpose shape={shape_progress}, refraction={refraction_progress}"
            );
        }
    });
}

#[test]
fn composed_contact_geometry_reaches_the_native_early_keyframes() {
    with_tab_shell(TabBackdrop::Flat(0.35), |shell| {
        assert!(shell.set_cursor(72.333, 61.0));
        assert!(shell.pointer_pressed_at_event_time(shell.exact_pointer_event_time(None)));
        for frame in 1..=12 {
            shell.update_after_exact_interval(std::time::Duration::from_nanos(16_666_667));
            if let Ok(root) = std::env::var("CRANPOSE_OPTICAL_DEBUG") {
                let capture = shell.renderer().capture_frame(1206, 360).unwrap();
                save_optical_capture(&root, &format!("contact-{frame:02}"), &capture);
            }
            let graph = shell.renderer().scene_mut().graph.as_ref().unwrap().clone();
            let bounds = rendered_lens_bounds(&graph.root, ProjectiveTransform::identity())
                .expect("rendered lens must be present");
            if let Some((_, width, height)) = [
                (3, 101.815, 60.315),
                (6, 109.544, 67.306),
                (9, 113.567, 70.912),
            ]
            .into_iter()
            .find(|sample| sample.0 == frame)
            {
                assert!(
                    (bounds.width - width).abs() <= 1.5,
                    "native width at frame {frame}: {bounds:?} expected {width}"
                );
                assert!(
                    (bounds.height - height).abs() <= 1.5,
                    "native height at frame {frame}: {bounds:?} expected {height}"
                );
            }
        }
    });
}

#[test]
fn composed_release_keeps_the_native_contact_clock_after_selection_commits() {
    with_tab_shell(TabBackdrop::Flat(0.35), |shell| {
        held_tab_capture(shell, TabBackdrop::Flat(0.35));
        assert!(shell.pointer_released_at_event_time(shell.exact_pointer_event_time(None)));
        for frame in 1..=12 {
            shell.update_after_exact_interval(std::time::Duration::from_nanos(16_666_667));
            let graph = shell.renderer().scene_mut().graph.as_ref().unwrap().clone();
            let lens = extract_lens(&graph.root, ProjectiveTransform::identity()).unwrap();
            let shader =
                terminal_effect_shader(lens.graphics_layer.backdrop_effect.as_ref().unwrap())
                    .unwrap();
            let u = shader.uniforms();
            let progress = (u[4] + u[5] - 149.0) / 32.0;
            if let Ok(root) = std::env::var("CRANPOSE_OPTICAL_DEBUG") {
                let capture = shell.renderer().capture_frame(1206, 360).unwrap();
                save_optical_capture(&root, &format!("release-{frame:02}"), &capture);
            }
            if let Some((_, expected)) = [(3, 0.762), (6, 0.273), (9, 0.108)]
                .into_iter()
                .find(|sample| sample.0 == frame)
            {
                assert!(
                    (progress - expected).abs() <= 0.08,
                    "committed release frame {frame}: native {expected}, Cranpose {progress}"
                );
            }
        }
    });
}

fn replay_tab_drag(trace: &str, mut inspect: impl FnMut(usize, f64, Rect, Rect, &[&str])) {
    for (route_index, route) in trace
        .split("start,")
        .filter(|route| !route.is_empty())
        .enumerate()
    {
        with_tab_shell(TabBackdrop::Flat(0.35), |shell| {
            let mut rows = route.lines();
            let initial: f32 = rows
                .next()
                .unwrap()
                .split(',')
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            shell.set_cursor(72.333 + initial, 61.0);
            if initial != 0.0 {
                assert!(shell.pointer_pressed());
                assert!(shell.pointer_released());
                for _ in 0..180 {
                    shell.update_after_exact_interval(std::time::Duration::from_nanos(16_666_667));
                }
            }
            let origin = shell.exact_pointer_event_time(None).animation_time_nanos;
            assert!(shell.pointer_pressed_at_event_time(shell.exact_pointer_event_time(None)));
            let mut frame_time = 0u64;
            for row in rows {
                let parts: Vec<_> = row.split(',').collect();
                let time = parts[1].parse::<f64>().unwrap();
                let nanos = (time * 1e9).round() as u64;
                let position: f32 = parts[2].parse().unwrap();
                if parts[0] == "up" {
                    assert!(shell.pointer_released_at_event_time(
                        cranpose_app_shell::PointerEventTime {
                            platform_time_ms: Some((nanos / 1_000_000) as i64),
                            animation_time_nanos: origin + nanos,
                        },
                    ));
                    continue;
                }
                if parts[0] == "input" {
                    assert!(shell.set_cursor_at_event_time(
                        72.333 + position,
                        61.0,
                        cranpose_app_shell::PointerEventTime {
                            platform_time_ms: Some((nanos / 1_000_000) as i64),
                            animation_time_nanos: origin + nanos,
                        },
                    ));
                    continue;
                }
                while nanos - frame_time > 20_000_000 {
                    shell.update_after_exact_interval(std::time::Duration::from_nanos(16_666_667));
                    frame_time += 16_666_667;
                }
                shell.update_after_exact_interval(std::time::Duration::from_nanos(
                    nanos - frame_time,
                ));
                frame_time = nanos;
                let graph = shell.renderer().scene_mut().graph.as_ref().unwrap().clone();
                let lens =
                    rendered_lens_bounds(&graph.root, ProjectiveTransform::identity()).unwrap();
                let pane = rendered_glass_bounds(&graph.root, ProjectiveTransform::identity(), 1.0)
                    .unwrap();
                let capture = shell.renderer().capture_frame(1206, 360).unwrap();
                if let Ok(root) = std::env::var("CRANPOSE_OPTICAL_DEBUG") {
                    save_optical_capture(&root, &format!("drag-{route_index}-{nanos}"), &capture);
                }
                inspect(route_index, time, lens, pane, &parts);
            }
        });
    }
}

#[test]
fn composed_drag_follows_the_native_input_and_frame_clock() {
    let mut checked = 0;
    let mut maximum = 0.0f32;
    let mut squared_error = 0.0f32;
    replay_tab_drag(
        include_str!("../../../cranpose-liquid/tests/fixtures/native_tab_drag.csv"),
        |_, _, lens, pane, parts| {
            let position: f32 = parts[2].parse().unwrap();
            let local = ((lens.x + lens.width * 0.5) - (pane.x + pane.width * 0.5))
                / (pane.width / 360.0)
                + 128.5;
            maximum = maximum.max((local - position).abs());
            squared_error += (local - position).powi(2);
            checked += 1;
        },
    );
    assert!(checked > 200);
    assert!(maximum <= 5.0, "maximum native drag error: {maximum}");
    let rms = (squared_error / checked as f32).sqrt();
    assert!(rms <= 1.5, "native drag RMS: {rms}");
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn composed_shape_matches_every_native_contact_drag_and_release_frame() {
    let mut counts = [0; 4];
    let mut squared_error = [0.0f32; 4];
    let mut maximum = [0.0f32; 4];
    let mut phases = [[0; 3]; 4];
    let mut samples = String::from("route,time,native_width,native_height,width,height,phase\n");
    replay_tab_drag(
        include_str!("../../../cranpose-liquid/tests/fixtures/native_tab_drag_geometry.csv"),
        |route, time, lens, pane, parts| {
            let expected = (
                parts[3].parse::<f32>().unwrap(),
                parts[4].parse::<f32>().unwrap(),
            );
            let actual = (
                lens.width / (pane.width / 360.0),
                lens.height / (pane.height / 62.0),
            );
            let phase = match parts[5] {
                "contact" => 0,
                "drag" => 1,
                "release" => 2,
                other => panic!("unexpected native phase: {other}"),
            };
            phases[route][phase] += 1;
            samples.push_str(&format!(
                "{route},{time},{},{},{},{},{}\n",
                expected.0, expected.1, actual.0, actual.1, parts[5]
            ));
            for error in [actual.0 - expected.0, actual.1 - expected.1] {
                squared_error[route] += error.powi(2);
                maximum[route] = maximum[route].max(error.abs());
            }
            counts[route] += 1;
        },
    );
    if let Ok(root) = std::env::var("CRANPOSE_OPTICAL_DEBUG") {
        std::fs::write(
            std::path::Path::new(&root).join("drag-geometry.csv"),
            samples,
        )
        .unwrap();
    }
    assert_eq!(counts, [229, 183, 183, 229]);
    assert!(phases.iter().flatten().all(|count| *count >= 35));
    for route in 0..4 {
        let rms = (squared_error[route] / (counts[route] * 2) as f32).sqrt();
        assert!(
            rms <= 1.0 && maximum[route] <= 3.0,
            "native contact/drag/release route {route}: {rms} pt RMS, {} pt maximum across {} frames",
            maximum[route],
            counts[route]
        );
    }
}

fn settled_tab_pixels(backdrop: TabBackdrop) -> Vec<u8> {
    with_tab_shell(backdrop, |shell| {
        held_tab_capture(shell, backdrop);
        assert!(shell.pointer_released());
        for _ in 0..240 {
            shell.update_after_exact_interval(std::time::Duration::from_millis(16));
        }
        shell.renderer().capture_frame(1206, 360).unwrap().pixels
    })
}

#[test]
fn settled_backdrop_replays_the_masked_foreground_pixels() {
    let backdrop = TabBackdrop::Foreground {
        accent: Color(0.0, 136.0 / 255.0, 1.0, 1.0),
        icon: 0,
    };
    with_tab_shell(backdrop, |shell| {
        held_tab_capture(shell, backdrop);
        assert!(shell.pointer_released());
        for _ in 0..240 {
            shell.update_after_exact_interval(std::time::Duration::from_millis(16));
        }
        let first = shell.renderer().capture_frame(1206, 360).unwrap();
        for repeat in 0..8 {
            let frame = shell.renderer().capture_frame(1206, 360).unwrap();
            assert!(
                frame.pixels == first.pixels,
                "settled frame {repeat} changed the cached composition"
            );
        }
    });
}

#[test]
fn settled_material_tint_stays_below_opaque_foreground() {
    let accent = [0.0, 136.0, 255.0];
    let background = settled_tab_pixels(TabBackdrop::Flat(1.0));
    let pixels = settled_tab_pixels(TabBackdrop::Foreground {
        accent: Color(0.0, 136.0 / 255.0, 1.0, 1.0),
        icon: 0,
    });
    for y in 135..150 {
        for x in 998..1008 {
            let offset = (y * 1206 + x) * 4;
            for channel in 0..3 {
                let expected = (f32::from(background[offset + channel]) * 0.5 + accent[channel]
                    - 127.5)
                    .clamp(0.0, 255.0);
                assert!(
                    (f32::from(pixels[offset + channel]) - expected).abs() <= 1.0,
                    "settled foreground at {x},{y}, channel {channel}: expected {expected}, actual {}",
                    pixels[offset + channel]
                );
            }
        }
    }
}

#[test]
fn foreground_edges_disperse_for_different_symbols_and_accent_channels() {
    let background = held_tab_pixels(TabBackdrop::Flat(1.0));
    for accent in [
        Color(0.2, 0.65, 0.1, 1.0),
        Color(0.1, 0.2, 0.65, 1.0),
        Color(0.65, 0.1, 0.2, 1.0),
    ] {
        for icon in 0..3 {
            let pixels = held_tab_pixels(TabBackdrop::Foreground { accent, icon });
            let mut weight = [0.0_f64; 3];
            let mut moment = [0.0_f64; 3];
            for y in 65..120 {
                for x in 930..1050 {
                    let offset = (y * 1206 + x) * 4;
                    for channel in 0..3 {
                        let absorption =
                            background[offset + channel].saturating_sub(pixels[offset + channel]);
                        weight[channel] += f64::from(absorption);
                        moment[channel] += f64::from(absorption) * x as f64;
                    }
                }
            }
            assert!(
                weight.iter().all(|value| *value > 1000.0),
                "each channel must contain visible foreground"
            );
            let centers =
                std::array::from_fn::<_, 3, _>(|channel| moment[channel] / weight[channel]);
            let spread = centers.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - centers.iter().copied().fold(f64::INFINITY, f64::min);
            assert!(
                spread > 1.0,
                "foreground {icon}, {accent:?}: RGB centers {centers:?}, spread {spread} px"
            );
        }
    }
}

#[test]
fn composed_tab_bar_keeps_every_icon_visible_above_its_frosted_surface() {
    let pixels = held_tab_pixels(TabBackdrop::Green);
    for (index, center) in [72, 158, 244, 330].into_iter().enumerate() {
        let mut ink = 0;
        let mut opaque_accent = 0;
        for y in 38 * 3..68 * 3 {
            for x in (center - 17) * 3..(center + 17) * 3 {
                let offset = (y * 1206 + x) * 4;
                let rgb = &pixels[offset..offset + 3];
                ink += usize::from(u32::from(rgb[0]) + u32::from(rgb[1]) + u32::from(rgb[2]) < 380);
                opaque_accent +=
                    usize::from(rgb[0] < 5 && rgb[1] > 60 && rgb[1] < 190 && rgb[2] > 160);
            }
        }
        assert!(ink > 30, "tab {index} must retain visible icon ink: {ink}");
        if index == 3 {
            assert!(
                opaque_accent > 30,
                "the native selected transfer keeps opaque red at zero: {opaque_accent}"
            );
        }
    }
}

fn held_tab_waves(horizontal: bool, period: f32) -> [Vec<u8>; 4] {
    std::array::from_fn(|phase| {
        held_tab_pixels(TabBackdrop::Wave {
            horizontal,
            period,
            phase: phase as f32 * std::f32::consts::FRAC_PI_2,
            channel: None,
        })
    })
}

#[test]
fn raised_tab_lens_preserves_native_backdrop_phase_on_both_sides_of_its_face() {
    let frames = held_tab_waves(true, 32.0);
    for (x, expected) in [(318usize, 318.062f32), (350, 350.062)] {
        let offset = (61 * 3 * 1206 + x * 3) * 4 + 1;
        let values: Vec<_> = frames
            .iter()
            .map(|frame| f32::from(frame[offset]))
            .collect();
        let cosine = values[0] - values[2];
        let sine = values[3] - values[1];
        assert!(
            cosine.hypot(sine) > 8.0,
            "probe must retain measurable detail"
        );
        let mut source = sine.atan2(cosine) * 32.0 / std::f32::consts::TAU;
        source += ((expected - source) / 32.0).round() * 32.0;
        assert!(
            (source - expected).abs() < 0.5,
            "native source {expected}, Cranpose source {source} at {x}; quadrature {values:?}"
        );
    }
}

#[test]
fn native_clear_rim_separates_wavelengths_across_the_edge() {
    let frames = held_tab_waves(true, 32.0);
    for (y, expected) in [
        (29, [331.832, 334.191, 336.403]),
        (93, [336.499, 334.136, 331.769]),
    ] {
        for (channel, expected) in expected.into_iter().enumerate() {
            let offset = (y * 3 * 1206 + 334 * 3) * 4 + channel;
            let values: Vec<_> = frames
                .iter()
                .map(|frame| f32::from(frame[offset]))
                .collect();
            let cosine = values[0] - values[2];
            let sine = values[3] - values[1];
            assert!(
                cosine.hypot(sine) * 0.5 > 25.0,
                "the native rim has a sharp optical response: {values:?}"
            );
            let mut source = sine.atan2(cosine) * 32.0 / std::f32::consts::TAU;
            source += ((334.0 - source) / 32.0).round() * 32.0;
            assert!(
                (source - expected).abs() <= 0.34,
                "rim channel {channel}, y={y}: native {expected}, Cranpose {source}"
            );
        }
    }
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn raised_tab_joins_bend_into_the_face_like_the_native_flat_probe() {
    let pixels = held_tab_pixels(TabBackdrop::Flat(0.35));
    let joins = [
        (304, 792.3333, 851.0),
        (310, 793.0, 850.6667),
        (334, 793.0, 850.6667),
    ]
    .map(|(x, native_top, native_bottom)| {
        let clear = |y: usize| pixels[(y * 1206 + x * 3) * 4 + 1] < 130;
        let top = (29 * 3..38 * 3)
            .rev()
            .find(|&y| clear(y))
            .expect("upper clear join") as f32
            / 3.0
            + 761.0;
        let bottom = (84 * 3..94 * 3)
            .find(|&y| clear(y))
            .expect("lower clear join") as f32
            / 3.0
            + 761.0;
        assert!(
            (top - native_top).abs() <= 0.34,
            "upper join at x={x}: Cranpose {top}, native {native_top}"
        );
        assert!(
            (bottom - native_bottom).abs() <= 0.34,
            "lower join at x={x}: Cranpose {bottom}, native {native_bottom}"
        );
        (top, bottom)
    });
    assert!(
        joins[1].0 > joins[0].0,
        "the upper join slopes down into the face"
    );
    assert!(
        joins[1].1 < joins[0].1,
        "the lower join slopes up into the face"
    );
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn resting_dark_selection_matches_the_native_solid_surface() {
    with_tab_shell(TabBackdrop::DarkFlat(0.0), |shell| {
        let frame = shell
            .renderer()
            .capture_frame(1206, 360)
            .expect("resting dark tab capture");
        for (x, y, expected) in [(130, 147, 53), (200, 227, 53), (800, 217, 19)] {
            let actual = frame.pixels[(y * 1206 + x) * 4];
            assert!(
                actual.abs_diff(expected) <= 1,
                "resting dark at {x},{y}: native {expected}, Cranpose {actual}"
            );
        }
    });
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn flat_native_controls_preserve_the_panes_uniform_face_response() {
    for (level, expected) in [(0.35, 196), (0.65, 237)] {
        let pixels = held_tab_pixels(TabBackdrop::Flat(level));
        for (x, y) in [(80, 32), (125, 39), (158, 61), (200, 79)] {
            let actual = pixels[(y * 3 * 1206 + x * 3) * 4 + 1];
            assert!(
                (i32::from(actual) - expected).abs() <= 1,
                "flat {level} at {x},{y}: native {expected}, Cranpose {actual}"
            );
        }
    }
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn native_pane_preserves_color_separately_from_brightness() {
    let samples = include_str!("fixtures/native_pane_color.csv")
        .lines()
        .skip(1)
        .map(|line| {
            line.split(',')
                .map(|value| value.parse::<usize>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(samples.len(), 36);
    let mut worst = 0;
    for channel in 0..3 {
        for phase in 0..4 {
            let pixels = held_tab_pixels(TabBackdrop::Wave {
                horizontal: true,
                period: 32.0,
                phase: phase as f32 * std::f32::consts::FRAC_PI_2,
                channel: Some(channel),
            });
            for row in samples
                .iter()
                .filter(|row| row[0] == channel && row[1] == phase)
            {
                for component in 0..3 {
                    let actual = pixels[(row[3] * 3 * 1206 + row[2] * 3) * 4 + component];
                    let difference = actual.abs_diff(row[4 + component] as u8);
                    worst = worst.max(difference);
                    if difference > 2 {
                        eprintln!(
                            "native color channel {channel}, phase {phase}, point {},{}, component {component}: expected {}, got {actual}",
                            row[2],
                            row[3],
                            row[4 + component]
                        );
                    }
                }
            }
        }
    }
    assert!(worst <= 2, "maximum native color difference: {worst}");
}

#[test]
fn raised_lens_matches_native_shadow_and_spectrum_over_the_same_pane_pixels() {
    let pixels = with_tab_shell(TabBackdrop::Flat(0.35), |shell| {
        held_tab_capture_with_input(shell, TabBackdrop::Flat(0.35), true)
    });
    let mut shadow_differences = 0;
    for row in include_str!("fixtures/native_pane/shadow.csv")
        .lines()
        .skip(1)
    {
        let values = row
            .split(',')
            .map(|value| value.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        for channel in 0..3 {
            let actual = pixels[(values[1] * 1206 + values[0]) * 4 + channel];
            assert!(
                (i32::from(actual) - values[2 + channel] as i32).abs() <= 2,
                "native input shadow at {},{}, channel {channel}: expected {}, observed {actual}",
                values[0],
                values[1],
                values[2 + channel]
            );
            shadow_differences +=
                usize::from(values[5 + channel].abs_diff(values[2 + channel]) > 2);
        }
    }
    assert!(
        shadow_differences > 0,
        "native ablation must expose the missing shadow"
    );
    for (horizontal, expected) in [
        (true, [32.19, 33.06, 31.60]),
        (false, [34.60, 34.70, 34.21]),
    ] {
        let frames = (0..4)
            .map(|phase| {
                let backdrop = TabBackdrop::Wave {
                    horizontal,
                    period: 32.0,
                    phase: phase as f32 * std::f32::consts::FRAC_PI_2,
                    channel: None,
                };
                with_tab_shell(backdrop, |shell| {
                    held_tab_capture_with_input(shell, backdrop, true)
                })
            })
            .collect::<Vec<_>>();
        for (channel, expected) in expected.into_iter().enumerate() {
            let offset = (29 * 3 * 1206 + 334 * 3) * 4 + channel;
            let cosine = f32::from(frames[0][offset]) - f32::from(frames[2][offset]);
            let sine = f32::from(frames[3][offset]) - f32::from(frames[1][offset]);
            let amplitude = cosine.hypot(sine) * 0.5;
            assert!(
                (amplitude - expected).abs() <= 1.0,
                "native input spectrum horizontal={horizontal} channel={channel}: native {expected}, Cranpose {amplitude}"
            );
        }
    }
}

#[test]
fn native_rim_filters_along_the_edge_and_preserves_normal_detail() {
    let mut errors = Vec::new();
    for (horizontal, period, expected) in [
        (true, 32.0, [32.19, 33.06, 31.60]),
        (true, 8.0, [8.54, 19.66, 8.60]),
        (false, 32.0, [34.60, 34.70, 34.21]),
        (false, 8.0, [34.30, 34.70, 34.40]),
    ] {
        let frames = held_tab_waves(horizontal, period);
        for (channel, expected) in expected.into_iter().enumerate() {
            let offset = (29 * 3 * 1206 + 334 * 3) * 4 + channel;
            let cosine = f32::from(frames[0][offset]) - f32::from(frames[2][offset]);
            let sine = f32::from(frames[3][offset]) - f32::from(frames[1][offset]);
            let amplitude = cosine.hypot(sine) * 0.5;
            if (amplitude - expected).abs() > 1.0 {
                errors.push(format!("horizontal={horizontal}, period={period}, channel={channel}: native {expected}, Cranpose {amplitude}"));
            }
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn native_rim_keeps_the_entering_image_visible_before_the_clear_return() {
    let pixels = held_tab_pixels(TabBackdrop::Flat(0.35));
    let actual = (25 * 3..29 * 3)
        .rev()
        .find(|&y| pixels[(y * 1206 + 334 * 3) * 4 + 1] > 130)
        .expect("entering image") as f32
        / 3.0
        + 761.0;
    assert!(
        (actual - 787.3333).abs() <= 0.34,
        "native entering image ends at 787.3333, Cranpose {actual}"
    );
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn native_lens_outline_preserves_the_corner_transition() {
    let pixels = held_tab_pixels(TabBackdrop::Flat(0.35));
    for (x, expected) in [
        (304, 787.0),
        (310, 786.3333),
        (334, 785.6667),
        (350, 785.6667),
        (370, 788.6667),
        (380, 795.0),
        (384, 799.0),
        (388, 805.0),
    ] {
        let actual = (24 * 3..61 * 3)
            .find(|&y| {
                pixels[(y * 1206 + x * 3) * 4..(y * 1206 + x * 3) * 4 + 3]
                    .iter()
                    .any(|&c| c > 130)
            })
            .expect("native outer lens outline") as f32
            / 3.0
            + 761.0;
        assert!(
            (actual - expected).abs() <= 0.34,
            "outline x={x}: native {expected}, Cranpose {actual}"
        );
    }
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn spectral_transmission_preserves_the_native_graded_color_band() {
    let pixels = held_tab_pixels(TabBackdrop::Flat(0.35));
    for (x, y, channel, expected) in [
        (334, 27, 0, 157),
        (334, 27, 1, 89),
        (389, 61, 2, 116),
        (386, 47, 1, 106),
        (386, 47, 2, 123),
    ] {
        let actual = pixels[(y * 3 * 1206 + x * 3) * 4 + channel];
        assert!(
            (i32::from(actual) - expected).abs() <= 5,
            "spectral band at {x},{y}, channel={channel}: native {expected}, Cranpose {actual}"
        );
    }
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn native_pane_edge_uses_one_coverage_ramp() {
    let pixels = held_tab_pixels(TabBackdrop::Flat(0.35));
    for (row, expected) in [
        (2369, 205),
        (2370, 231),
        (2371, 218),
        (2372, 196),
        (2559, 196),
        (2560, 218),
        (2561, 231),
        (2562, 204),
    ] {
        let actual = pixels[((row - 2283) * 1206 + 474) * 4 + 1];
        assert!(
            (i32::from(actual) - expected).abs() <= 1,
            "pane edge row={row}: native {expected}, Cranpose {actual}"
        );
    }
}

#[test]
#[ignore = "unmet native parity target; run just audit-liquid-native-parity"]
fn raised_tab_light_preserves_the_native_face_illumination() {
    let pixels = held_tab_pixels(TabBackdrop::Flat(0.35));
    for (x, y, expected) in [
        (280, 44, 195),
        (300, 44, 195),
        (330, 44, 196),
        (360, 44, 196),
        (280, 71, 194),
        (300, 71, 195),
        (330, 71, 196),
        (360, 71, 196),
    ] {
        let actual = pixels[(y * 3 * 1206 + x * 3) * 4 + 1];
        assert!(
            (i32::from(actual) - expected).abs() <= 2,
            "native raised light at {x},{y}: expected {expected}, observed {actual}"
        );
    }
}

#[test]
fn native_touch_light_uses_a_gaussian_disk() {
    let source = include_str!("../../../cranpose-liquid/src/widgets/tab_lighting.wgsl")
        .replace("fn effect_fs(", "fn lighting_fs(");
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n{source}\n@fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
            let p = input.position.xy - vec2<f32>(160.0);
            return vec4<f32>(vec3<f32>(blurred_disk(length(p), 46.5)), 1.0);
        }}"
    ));
    shader.set_float(0, 1.0);
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 320.0,
    };
    let graph = support::shader_probe_graph(bounds, shader);
    let mut renderer = support::headless_renderer().expect("GPU native light test must run");
    let frame = support::capture_graph(&mut renderer, graph, 320, 320);
    for row in include_str!("fixtures/native_glow_mask.csv")
        .lines()
        .skip(1)
    {
        let values = row
            .split(',')
            .map(|v| v.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        let actual = frame.pixels[((values[1] + 160) * 320 + values[0] + 160) * 4];
        assert!(
            (i32::from(actual) - values[2] as i32).abs() <= 2,
            "native glow at {},{}: expected {}, observed {actual}",
            values[0],
            values[1],
            values[2]
        );
    }
}
