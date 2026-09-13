mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{
    Color, GraphicsLayer, Rect, RenderEffect, RuntimeShader, liquid_glass_runtime_effect,
};

struct NativeSpectralSample {
    pixel: usize,
    distance: f32,
    normal: [f32; 2],
    rgb: [u8; 3],
}

#[test]
fn adaptive_pane_tone_matches_native_light_and_dark_boundaries() {
    let samples = include_str!("fixtures/native_pane_tone.csv")
        .lines()
        .skip(1)
        .map(|line| {
            line.split(',')
                .map(|value| value.parse::<f32>().expect("native tone sample"))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(samples.len(), 48);
    let fields = samples
        .iter()
        .map(|v| format!("vec2<f32>({}, {})", v[0], v[1]))
        .collect::<Vec<_>>()
        .join(",");
    let source =
        cranpose_ui_graphics::LIQUID_GLASS_WGSL.replace("fn effect_fs(", "fn material_fs(");
    let shader = RuntimeShader::new(&format!(
        "{source}\nconst samples = array<vec2<f32>, {}>({fields});
        @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
            let index = min(u32(input.uv.x * {}.0), {}u);
            let sample = samples[index / 3u];
            let encoded = backdrop_tone_curve(sample.x, sample.y)[index % 3u] * 255.0;
            return vec4<f32>(floor(encoded) / 255.0, fract(encoded), 0.0, 1.0);
        }}",
        samples.len(),
        samples.len() * 3,
        samples.len() * 3 - 1
    ));
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: (samples.len() * 3) as f32,
        height: 1.0,
    };
    let mut renderer = support::headless_renderer().expect("adaptive tone requires GPU");
    renderer.scene_mut().graph = Some(support::shader_probe_graph(bounds, shader));
    let frame = renderer
        .capture_frame((samples.len() * 3) as u32, 1)
        .expect("adaptive tone capture");
    for (index, sample) in samples.iter().enumerate() {
        let expected = [
            (sample[3] - sample[2]) * (1.0 - sample[5]),
            sample[2] * (1.0 - sample[5]) + sample[4] * sample[5],
            1.0 - sample[5],
        ];
        for (channel, expected) in expected.into_iter().enumerate() {
            let pixel = &frame.pixels[(index * 3 + channel) * 4..];
            let actual = (f32::from(pixel[0]) + f32::from(pixel[1]) / 255.0) / 255.0;
            assert!(
                (actual - expected).abs() <= 0.00003,
                "native tone sample {index}, channel {channel}: {sample:?}, expected {expected}, actual {actual}"
            );
        }
    }
}

#[test]
fn adaptive_key_fill_matches_native_light_and_dark_color_matrices() {
    let samples = include_str!("fixtures/native_pane_tone.csv")
        .lines()
        .skip(1)
        .map(|line| {
            line.split(',')
                .map(|value| value.parse::<f32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let colors = [[0.1, 0.12, 0.08], [0.35, 0.45, 0.38], [0.7, 0.8, 0.65]];
    let fields = samples
        .iter()
        .map(|v| format!("vec2<f32>({}, {})", v[0], v[1]))
        .collect::<Vec<_>>()
        .join(",");
    let color_fields = colors
        .iter()
        .map(|v| format!("vec3<f32>({}, {}, {})", v[0], v[1], v[2]))
        .collect::<Vec<_>>()
        .join(",");
    let source =
        cranpose_ui_graphics::LIQUID_GLASS_WGSL.replace("fn effect_fs(", "fn material_fs(");
    let shader = RuntimeShader::new(&format!("{source}
        const samples = array<vec2<f32>, {}>({fields});
        const colors = array<vec3<f32>, 3>({color_fields});
        @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
            let index = min(u32(input.uv.x * {}.0), {}u);
            let sample = samples[index / 3u];
            return vec4<f32>(adaptive_key_fill_color(colors[index % 3u], backdrop_tone_curve(sample.x, sample.y)), 1.0);
        }}", samples.len(), samples.len() * 3, samples.len() * 3 - 1));
    let width = (samples.len() * 3) as u32;
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: 1.0,
    };
    let mut renderer = support::headless_renderer().expect("native key/fill requires GPU");
    renderer.scene_mut().graph = Some(support::shader_probe_graph(bounds, shader));
    let frame = renderer.capture_frame(width, 1).expect("key/fill capture");
    assert_eq!(samples.len(), 48);
    for (index, sample) in samples.iter().enumerate() {
        for (color_index, color) in colors.iter().enumerate() {
            for channel in 0..3 {
                let matrix = &sample[6 + channel * 4..];
                let expected = (matrix[0] * color[0]
                    + matrix[1] * color[1]
                    + matrix[2] * color[2]
                    + matrix[3])
                    .clamp(0.0, 1.0)
                    * 255.0;
                let actual = frame.pixels[(index * 3 + color_index) * 4 + channel];
                assert!(
                    (f32::from(actual) - expected).abs() <= 1.0,
                    "key/fill sample {index}, color {color_index}, channel {channel}: expected {expected}, actual {actual}"
                );
            }
        }
    }
}

#[test]
fn native_pane_transmission_preserves_the_measured_inward_profile() {
    let samples = include_str!("fixtures/native_pane_transmission.csv")
        .lines()
        .skip(1)
        .map(|line| {
            let v = line
                .split(',')
                .map(|value| value.parse::<f32>().unwrap())
                .collect::<Vec<_>>();
            [v[0], 0.0, v[1], v[2] * v[1]]
        })
        .collect::<Vec<_>>();
    assert_native_pane_transmission(&samples, 49.6, 0.35);
}

#[test]
fn native_ios27_pane_transmission_matches_both_cardinal_axes() {
    let samples = include_str!("fixtures/native_pane_transmission_ios27.csv")
        .lines()
        .skip(1)
        .map(|line| {
            line.split(',')
                .map(|value| value.parse::<f32>().unwrap())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        })
        .collect::<Vec<[f32; 4]>>();
    assert_eq!(samples.len(), 44);
    assert_native_pane_transmission(&samples, 31.0, 0.14);
}

fn assert_native_pane_transmission(samples: &[[f32; 4]], reach: f32, tolerance: f32) {
    let fields = samples
        .iter()
        .map(|v| format!("vec3<f32>({}, {}, {})", v[0], v[1], v[2]))
        .collect::<Vec<_>>()
        .join(",");
    let source =
        cranpose_ui_graphics::LIQUID_GLASS_WGSL.replace("fn effect_fs(", "fn material_fs(");
    let mut shader = RuntimeShader::new(&format!(
        "{source}\nconst samples = array<vec3<f32>, {}>({fields});
        @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
            let sample = samples[min(u32(input.uv.x * {}.0), {}u)];
            let displacement = channel_lens_displacement(sample.yz * 31.0, vec2<f32>(180.0, 31.0), -sample.x, 15.5, 1.0, 0.8, 1.0, 1.0, vec2<f32>(0.0), 0.0, 0.0, 1.0, vec2<f32>(0.0));
            let encoded = (dot(displacement, sample.yz) + 64.0) / 128.0 * 255.0;
            return vec4<f32>(floor(encoded) / 255.0, fract(encoded), 0.0, 1.0);
        }}", samples.len(), samples.len(), samples.len()-1
    ));
    shader.set_float(130, 1.0);
    shader.set_float(131, reach);
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: samples.len() as f32,
        height: 1.0,
    };
    let mut renderer = support::headless_renderer().expect("native pane probe requires GPU");
    renderer.scene_mut().graph = Some(support::shader_probe_graph(bounds, shader));
    let frame = renderer
        .capture_frame(samples.len() as u32, 1)
        .expect("pane probe capture");
    for (index, expected) in samples.iter().enumerate() {
        let pixel = &frame.pixels[index * 4..index * 4 + 4];
        let displacement =
            (f32::from(pixel[0]) + f32::from(pixel[1]) / 255.0) / 255.0 * 128.0 - 64.0;
        assert!(
            (displacement - expected[3]).abs() <= tolerance,
            "pane depth {}, normal ({}, {}): native {}, Cranpose {displacement}",
            expected[0],
            expected[1],
            expected[2],
            expected[3]
        );
    }
}

fn assert_native_spectral_step(horizontal: bool, phase: u8, samples: &[NativeSpectralSample]) {
    assert_native_spectral_profile(horizontal, phase, samples, None);
}

fn assert_native_spectral_profile(
    horizontal: bool,
    phase: u8,
    samples: &[NativeSpectralSample],
    activity: Option<f32>,
) {
    let mut renderer = support::headless_renderer().expect("spectral kernel test requires GPU");
    let source =
        cranpose_ui_graphics::LIQUID_GLASS_WGSL.replace("fn effect_fs(", "fn material_fs(");
    let fields = samples
        .iter()
        .map(|sample| {
            format!(
                "vec3<f32>({}, {}, {})",
                sample.distance, sample.normal[0], sample.normal[1]
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut shader = RuntimeShader::new(&format!(
        "{source}\nconst fields = array<vec3<f32>, 42>({fields});
        @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
            let size = logical_extent();
            let index = u32(floor(select(input.uv.y * size.y, input.uv.x * size.x, {horizontal})));
            let measured = fields[min(index, 41u)];
            let scene = GlassScene(vec2<f32>(0.0), size, 0.0, 0u, vec2<f32>(1.0), 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, vec2<f32>(1.0, 0.0), 1.0, 1.0);
            let field = EdgeLensRay(scene, vec2<f32>(0.0), size, measured.x, 112.24138, 1.0, 1.0, vec2<f32>(0.0), measured.yz, 0.0);
            let spectrum = edge_spectrum(field.lens_refraction, 4.09435, 1.0);
            let dispersed = sample_chromatic_edge_lens(region_map(), input.uv, size, field, spectrum);
            let source_color = textureSampleLevel(input_texture, input_sampler, input.uv, 0.0).rgb;
            let opacity = spectral_presence(spectrum, field.distance);
            return vec4<f32>(mix(source_color, dispersed, opacity), 1.0);
        }}"
    ));
    if let Some(activity) = activity {
        let values = [
            std::f32::consts::FRAC_PI_2 - std::f32::consts::PI * 7.0 / 12.0 * activity,
            4.068 / (std::f32::consts::PI / 12.0).cos() * (-5.0 + 8.684211 * activity) / 3.6842105,
            0.8265,
            activity,
            1.0 - activity,
            14.0 * activity * (112.24138 / 36.0),
            38.88889 * activity * (112.24138 / 36.0),
            1.0,
        ];
        for (index, value) in values.into_iter().enumerate() {
            shader.set_float(148 + index, value);
        }
    }
    let (width, height) = if horizontal { (42, 4) } else { (4, 42) };
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let white = if horizontal {
        Rect {
            x: 21.0,
            width: 21.0,
            ..bounds
        }
    } else {
        Rect {
            y: 21.0,
            height: 21.0,
            ..bounds
        }
    };
    let filter = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::runtime_shader(shader)),
            ..Default::default()
        },
        vec![],
    );
    let edge_level = (255.0 - f32::from(phase) * 64.0) / 255.0;
    let edge = if horizontal {
        Rect {
            x: 21.0,
            width: 1.0,
            ..bounds
        }
    } else {
        Rect {
            y: 21.0,
            height: 1.0,
            ..bounds
        }
    };
    renderer.scene_mut().graph = Some(RenderGraph::new(shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            support::solid_rect(bounds, Color::BLACK),
            support::solid_rect(white, Color::WHITE),
            support::solid_rect(edge, Color(edge_level, edge_level, edge_level, 1.0)),
            RenderNode::Layer(Box::new(filter)),
        ],
    )));
    let frame = renderer
        .capture_frame(width, height)
        .expect("spectral step capture");
    assert_eq!(samples.len(), 42);
    for sample in samples {
        let index = if horizontal {
            width as usize + sample.pixel
        } else {
            sample.pixel * width as usize + 1
        };
        for channel in 0..3 {
            let actual = frame.pixels[index * 4 + channel];
            let expected = sample.rgb[channel];
            assert!(
                (i32::from(actual) - i32::from(expected)).abs() <= 2,
                "native spectral step horizontal={horizontal}, phase={phase}, activity={activity:?}, pixel={}, channel={channel}: native {expected}, actual {actual}",
                sample.pixel
            );
        }
    }
}

#[test]
fn native_contact_spectrum_reverses_direction_and_changes_depth_opacity() {
    let rows = include_str!("fixtures/native_contact_spectrum.csv")
        .lines()
        .skip(1)
        .map(|row| {
            row.split(',')
                .map(|value| value.parse::<f32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for phase in 0..5 {
        for horizontal in [true, false] {
            let samples = rows
                .iter()
                .filter(|row| row[0] == phase as f32 && (row[1] == 1.0) == horizontal)
                .map(|row| NativeSpectralSample {
                    pixel: row[2] as usize,
                    distance: row[3],
                    normal: [row[4], row[5]],
                    rgb: [row[6] as u8, row[7] as u8, row[8] as u8],
                })
                .collect::<Vec<_>>();
            assert_native_spectral_profile(horizontal, 0, &samples, Some(phase as f32 * 0.25));
        }
    }
}

#[test]
fn native_spectral_step_preserves_seven_tap_weights_and_depth_opacity() {
    let samples = include_str!("fixtures/native_spectral_step.csv")
        .lines()
        .skip(1)
        .map(|row| {
            let values: Vec<usize> = row
                .split(',')
                .map(|value| value.parse().expect("native step value"))
                .collect();
            NativeSpectralSample {
                pixel: values[0],
                distance: -0.623563,
                normal: [0.0, -1.0],
                rgb: [values[1] as u8, values[2] as u8, values[3] as u8],
            }
        })
        .collect::<Vec<_>>();
    assert_native_spectral_step(true, 0, &samples);
}

#[test]
fn native_vertical_spectral_step_preserves_directional_sample_spacing() {
    for phase in 0..4 {
        let samples = include_str!("fixtures/native_spectral_vertical_step.csv")
            .lines()
            .skip(1)
            .map(|row| {
                row.split(',')
                    .map(|value| value.parse::<f32>().expect("native vertical step value"))
                    .collect::<Vec<_>>()
            })
            .filter(|values| values[0] as u8 == phase)
            .map(|values| NativeSpectralSample {
                pixel: values[1] as usize,
                distance: values[2],
                normal: [values[3], values[4]],
                rgb: [values[5] as u8, values[6] as u8, values[7] as u8],
            })
            .collect::<Vec<_>>();
        assert_native_spectral_step(false, phase, &samples);
    }
}

fn striped_surface(normal: bool, specialized: bool) -> RenderGraph {
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 160.0,
        height: 96.0,
    };
    let mut children = (0..20)
        .map(|index| {
            support::solid_rect(
                Rect {
                    x: index as f32 * 8.0,
                    width: 8.0,
                    ..viewport
                },
                if index % 2 == 0 {
                    Color(0.9, 0.1, 0.1, 1.0)
                } else {
                    Color(0.1, 0.9, 0.1, 1.0)
                },
            )
        })
        .collect::<Vec<_>>();
    let mut shader = RuntimeShader::new(cranpose_ui_graphics::LIQUID_GLASS_WGSL);
    for (slot, value) in [
        (6, -1.0),
        (18, 1.0),
        (24, 1.0),
        (94, 0.75),
        (96, 1.0),
        (98, 16.0),
        (99, 1.0),
        (101, 1.0),
        (111, 1.0),
        (130, f32::from(normal)),
        (131, 32.0),
    ] {
        shader.set_float(slot, value);
    }
    children.push(RenderNode::Layer(Box::new(
        shared_test_support::layer_node(
            Rect {
                x: 8.0,
                y: 16.0,
                width: 144.0,
                height: 64.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                backdrop_effect: Some(if specialized {
                    liquid_glass_runtime_effect(shader)
                } else {
                    RenderEffect::runtime_shader(shader)
                }),
                ..Default::default()
            },
            vec![],
        ),
    )));
    RenderGraph::new(shared_test_support::layer_node(
        viewport,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

#[test]
fn resting_blur_crossfades_one_kernel_and_keeps_foreground_sharp() {
    let mut renderer = support::headless_renderer().expect("resting blur requires GPU");
    let frames = [0.0, 0.25, 0.5, 1.0].map(|opacity| {
        let mut graph = striped_surface(false, false);
        let RenderNode::Layer(layer) = graph.root.children.last_mut().unwrap() else {
            panic!("expected glass layer");
        };
        let Some(RenderEffect::Shader { shader }) = &layer.graphics_layer.backdrop_effect else {
            panic!("expected source shader");
        };
        let mut shader = (**shader).clone();
        shader.set_float(130, 2.0);
        shader.set_float(131, 0.0);
        shader.set_float(132, 1.0);
        shader.set_float2(
            cranpose_ui_graphics::GLASS_BACKDROP_BLUR_UNIFORM,
            6.0,
            opacity,
        );
        layer.graphics_layer.backdrop_effect = Some(liquid_glass_runtime_effect(shader));
        graph.root.children.push(support::solid_rect(
            Rect {
                x: 70.0,
                y: 40.0,
                width: 8.0,
                height: 8.0,
            },
            Color::BLUE,
        ));
        renderer.scene_mut().graph = Some(graph);
        renderer
            .capture_frame(160, 96)
            .expect("resting blur capture")
    });
    let mut signal = 0;
    for y in 32..60 {
        for x in 40..120 {
            let offset = (y * 160 + x) * 4;
            for channel in 0..4 {
                let clear = f32::from(frames[0].pixels[offset + channel]);
                let blurred = f32::from(frames[3].pixels[offset + channel]);
                signal += usize::from((clear - blurred).abs() > 10.0);
                for (index, opacity) in [(1, 0.25), (2, 0.5)] {
                    let expected = clear + (blurred - clear) * opacity;
                    let actual = f32::from(frames[index].pixels[offset + channel]);
                    assert!(
                        (actual - expected).abs() <= 2.0,
                        "a fixed blur kernel must crossfade linearly at ({x}, {y}): {actual} != {expected}"
                    );
                }
            }
        }
    }
    assert!(signal > 500, "the blur must change the striped backdrop");
    for frame in &frames {
        for y in 40..48 {
            for x in 70..78 {
                assert_eq!(&frame.pixels[(y * 160 + x) * 4..][..4], &[0, 0, 255, 255]);
            }
        }
    }
}

#[test]
fn straight_surface_edges_preserve_tangential_background_positions() {
    let mut renderer = support::headless_renderer().expect("GPU surface refraction test must run");
    for normal in [false, true] {
        renderer.scene_mut().graph = Some(striped_surface(normal, true));
        let frame = renderer.capture_frame(160, 96).expect("surface capture");
        let mut changed = 0;
        for y in [21, 23, 73, 75] {
            for x in (44..116).step_by(8) {
                let offset = (y * 160 + x) * 4;
                let is_red = frame.pixels[offset] > frame.pixels[offset + 1];
                changed += usize::from(is_red != (x / 8 % 2 == 0));
            }
        }
        if normal {
            assert_eq!(
                changed, 0,
                "the surface must bend perpendicular to its straight edges"
            );
            let bent = [12, 16, 20]
                .into_iter()
                .filter(|&x| {
                    let offset = (48 * 160 + x) * 4;
                    let red = frame.pixels[offset] > frame.pixels[offset + 1];
                    red != (x / 8 % 2 == 0)
                })
                .count();
            assert!(
                bent > 0,
                "the curved cap must actually displace the background"
            );
        } else {
            assert!(
                changed > 12,
                "radial counterexample must bend the horizontal coordinate"
            );
        }
    }
}

#[test]
fn surface_refraction_specialization_preserves_pixels() {
    let mut renderer = support::headless_renderer().expect("GPU surface refraction test must run");
    renderer.scene_mut().graph = Some(striped_surface(true, false));
    let general = renderer
        .capture_frame(160, 96)
        .expect("general surface capture");
    renderer.scene_mut().graph = Some(striped_surface(true, true));
    let specialized = renderer
        .capture_frame(160, 96)
        .expect("specialized surface capture");
    assert_eq!(general.pixels, specialized.pixels);
}

fn configure_glass_folds(effect: &mut RenderEffect, enabled: bool) {
    match effect {
        RenderEffect::Shader { shader } => {
            let shader = std::sync::Arc::make_mut(shader);
            cranpose_ui_graphics::specialize_liquid_glass_with_folds(shader, enabled);
            if matches!(
                shader
                    .uniforms()
                    .get(cranpose_ui_graphics::GLASS_OPTICAL_STAGE_UNIFORM),
                Some(1.0 | 2.0)
            ) {
                shader.set_draw_split(None);
            }
        }
        RenderEffect::Chain { first, second } => {
            configure_glass_folds(std::sync::Arc::make_mut(first), enabled);
            configure_glass_folds(std::sync::Arc::make_mut(second), enabled);
        }
        _ => panic!("expected a glass shader chain"),
    }
}

fn exterior_lens(reach: f32, specialized: bool, recolor: bool) -> RenderGraph {
    let mut graph = striped_surface(false, specialized);
    let children = &mut graph.root.children;
    let RenderNode::Layer(lens) = children.last_mut().expect("lens") else {
        panic!("lens must be a layer");
    };
    let mut shader = RuntimeShader::new(cranpose_ui_graphics::LIQUID_GLASS_WGSL);
    shader.set_input_padding(24.0);
    for (slot, value) in [
        (6, -1.0),
        (18, 1.0),
        (24, 1.0),
        (89, 1.2),
        (94, 0.25),
        (96, 1.0),
        (98, 8.0),
        (99, 1.0),
        (101, 1.0),
        (111, 1.0),
        (130, 2.0),
        (131, reach),
    ] {
        shader.set_float(slot, value);
    }
    if recolor {
        shader.set_float4(124, 0.0, 0.5, 1.0, 1.0);
    }
    let mut effect = liquid_glass_runtime_effect(shader);
    configure_glass_folds(&mut effect, specialized);
    lens.graphics_layer.backdrop_effect = Some(effect);
    let face = support::solid_rect(
        Rect {
            x: 8.0,
            y: 16.0,
            width: 144.0,
            height: 64.0,
        },
        Color(0.1, 0.1, 0.8, 1.0),
    );
    children.insert(children.len() - 1, face);
    let ink = support::solid_rect(
        Rect {
            x: 60.0,
            y: 50.0,
            width: 40.0,
            height: 4.0,
        },
        Color::BLACK,
    );
    children.insert(children.len() - 1, ink);
    graph
}

#[test]
fn raised_edge_lens_refracts_exterior_without_warping_face_ink() {
    let mut renderer = support::headless_renderer().expect("GPU edge lens test must run");
    renderer.scene_mut().graph = Some(exterior_lens(0.0, true, false));
    let flat = renderer.capture_frame(160, 96).expect("flat face capture");
    renderer.scene_mut().graph = Some(exterior_lens(20.0, true, false));
    let raised = renderer
        .capture_frame(160, 96)
        .expect("raised lens capture");
    let exterior = exterior_pixels(&raised, 17..20);
    assert_eq!(exterior_pixels(&flat, 17..20), 0);
    assert!(
        exterior >= 56,
        "the refracted rim must return exterior: {exterior}/168"
    );
    for y in 36..60 {
        for x in 52..108 {
            let offset = (y * 160 + x) * 4;
            assert_eq!(
                &flat.pixels[offset..offset + 4],
                &raised.pixels[offset..offset + 4],
                "rim must leave face ink unchanged at {x},{y}"
            );
        }
    }
    renderer.scene_mut().graph = Some(exterior_lens(20.0, false, false));
    let general = renderer
        .capture_frame(160, 96)
        .expect("general lens capture");
    assert_eq!(general.pixels, raised.pixels);
}

#[test]
fn raised_lens_reads_the_page_outside_the_lifted_surface() {
    let mut graph = exterior_lens(20.0, true, false);
    let content = graph.root.children.drain(20..22).collect();
    let mut surface = shared_test_support::layer_node(
        Rect {
            x: 8.0,
            y: 16.0,
            width: 144.0,
            height: 64.0,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer {
            scale_x: 1.04,
            scale_y: 1.02,
            ..Default::default()
        },
        content,
    );
    surface.transform_to_parent = ProjectiveTransform::from_rect_to_quad(
        surface.local_bounds,
        cranpose_render_common::layer_transform::apply_layer_to_quad(
            surface.local_bounds,
            surface.local_bounds,
            &surface.graphics_layer,
        ),
    );
    graph
        .root
        .children
        .insert(20, RenderNode::Layer(Box::new(surface)));
    let mut renderer = support::headless_renderer().expect("GPU lifted surface test must run");
    renderer.scene_mut().graph = Some(RenderGraph::new(graph.root));
    let frame = renderer
        .capture_frame(160, 96)
        .expect("lifted surface capture");
    let exterior = exterior_pixels(&frame, 19..23);
    assert!(
        exterior > 160,
        "lens must read page beyond lifted surface: {exterior}/224"
    );
}

#[test]
fn face_ink_recolor_preserves_the_exterior_rainbow() {
    let mut renderer = support::headless_renderer().expect("GPU edge color test must run");
    renderer.scene_mut().graph = Some(exterior_lens(20.0, true, false));
    let plain = renderer.capture_frame(160, 96).expect("plain lens capture");
    renderer.scene_mut().graph = Some(exterior_lens(20.0, true, true));
    let accent = renderer
        .capture_frame(160, 96)
        .expect("accent lens capture");
    for y in 19..22 {
        for x in 52..108 {
            let offset = (y * 160 + x) * 4;
            assert_eq!(
                &plain.pixels[offset..offset + 4],
                &accent.pixels[offset..offset + 4],
                "page colors at the rim are not caption ink: {x},{y}"
            );
        }
    }
    let ink = (53 * 160 + 80) * 4;
    assert!(plain.pixels[ink + 2] < 10);
    assert!(accent.pixels[ink + 2] > 240);
}

fn exterior_pixels(
    frame: &cranpose_render_wgpu::CapturedFrame,
    rows: std::ops::Range<usize>,
) -> usize {
    let mut exterior = 0;
    for y in rows {
        for x in 52..108 {
            let offset = (y * 160 + x) * 4;
            let pixel = &frame.pixels[offset..offset + 3];
            exterior += usize::from(u16::from(pixel[0].max(pixel[1])) > u16::from(pixel[2]) + 30);
        }
    }
    exterior
}

fn touch_glow_scene(density: f32, pressed: bool) -> RenderGraph {
    use cranpose_liquid::{Glass, GlassDynamics, LiquidColors};
    let mut graph = striped_surface(true, true);
    let mut surface = graph.root.children.pop().expect("glass surface");
    let RenderNode::Layer(layer) = &mut surface else {
        panic!("glass surface must be a layer");
    };
    layer.graphics_layer.backdrop_effect = Some(
        Glass::clear()
            .no_clip()
            .shadow(false)
            .face_lighting(false)
            .blur_radius(0.0)
            .tint(Color::TRANSPARENT)
            .saturation(1.0)
            .lift(0.0)
            .contrast(1.0)
            .highlight(0.0)
            .transmission_refraction(0.0)
            .backdrop_effect(
                &LiquidColors::light(Color::BLACK),
                density,
                GlassDynamics {
                    touch: pressed.then_some((96.0, 32.0, 1.0)),
                    touch_radius_dp: Some(14.0),
                    ..Default::default()
                },
            ),
    );
    graph.root.children = vec![
        support::solid_rect(graph.root.local_bounds, Color(0.4, 0.4, 0.4, 1.0)),
        surface,
    ];
    graph
}

#[test]
fn cover_glass_touch_coordinates_and_radius_follow_display_density() {
    let mut renderer = support::headless_renderer().expect("GPU touch glow test must run");
    for scale in [1u32, 3] {
        renderer.set_root_scale(scale as f32);
        let frames: Vec<_> = [false, true]
            .map(|pressed| {
                renderer.scene_mut().graph = Some(touch_glow_scene(scale as f32, pressed));
                renderer
                    .capture_frame(160 * scale, 96 * scale)
                    .expect("touch glow capture")
            })
            .into_iter()
            .collect();
        for (x, expected) in [(104, 23), (122, 0)] {
            let offset = ((48 * scale * 160 * scale + x * scale) * 4) as usize;
            let difference =
                i32::from(frames[1].pixels[offset]) - i32::from(frames[0].pixels[offset]);
            assert!(
                (difference - expected).abs() <= 1,
                "touch glow at density {scale}, x={x}: {difference}, expected {expected}"
            );
        }
    }
}
