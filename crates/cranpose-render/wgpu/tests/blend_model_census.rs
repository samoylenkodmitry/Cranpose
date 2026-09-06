mod support;

use std::fmt::Write as _;

use half::f16;

const SIZE: u32 = 256;
const PIXELS: usize = (SIZE * SIZE) as usize;

const SHADER: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}
"#;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
}

fn gpu() -> Option<Gpu> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("blend census"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some(Gpu {
        device,
        queue,
        name: format!("{} ({:?})", info.name, info.backend),
    })
}

/// Column 0 is a zero premultiplied source, row 255 an opaque one; the rest
/// puts every channel at a different fraction of an 8-bit step so that ties
/// and near-ties of the blend land on every value.
fn source(x: u32, y: u32) -> [f32; 4] {
    if x == 0 {
        return [0.0; 4];
    }
    if y == SIZE - 1 {
        return [(x as f32 + 0.5) / 255.0, 1.0, x as f32 / 255.0, 1.0];
    }
    [
        (x as f32 + 0.5) / 255.0,
        (y as f32 + 0.25) / 255.0,
        x as f32 / 255.0,
        (y as f32 + 0.5) / 255.0,
    ]
}

fn destination_unorm(x: u32, y: u32) -> [u8; 4] {
    [
        x as u8,
        y as u8,
        ((x * 7 + y * 3) & 255) as u8,
        (255 - (x + y) / 2) as u8,
    ]
}

fn destination_half(x: u32, y: u32) -> [f16; 4] {
    let [r, g, b, a] = destination_unorm(x, y);
    [
        f16::from_f32((f32::from(r) + 0.5) / 255.0),
        f16::from_f32(f32::from(g) / 255.0),
        f16::from_f32((f32::from(b) + 0.25) / 255.0),
        f16::from_f32(f32::from(a) / 255.0),
    ]
}

impl Gpu {
    fn texture(&self, format: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    fn upload(&self, texture: &wgpu::Texture, bytes: &[u8]) {
        let bytes_per_texel = texture.format().block_copy_size(None).expect("colour");
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * bytes_per_texel),
                rows_per_image: Some(SIZE),
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
    }

    /// The attachment's bytes after one full-screen premultiplied src-over
    /// draw of `source` over `destination`, both uploaded bit for bit.
    fn blend(&self, format: wgpu::TextureFormat, destination: &[u8]) -> Vec<u8> {
        let target = self.texture(
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
        );
        self.upload(&target, destination);
        let source_texture = self.texture(
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let mut source_bytes = Vec::with_capacity(PIXELS * 16);
        for y in 0..SIZE {
            for x in 0..SIZE {
                for channel in source(x, y) {
                    source_bytes.extend_from_slice(&channel.to_le_bytes());
                }
            }
        }
        self.upload(&source_texture, &source_bytes);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("blend census"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blend census"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &source_texture.create_view(&Default::default()),
                ),
            }],
        });
        let view = target.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blend census"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        support::read_texture(&self.device, &self.queue, &target)
    }
}

fn round_half_up(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

fn round_half_even(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8
}

fn over(s: [f64; 4], d: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|i| s[i] + d[i] * (1.0 - s[3]))
}

fn quantized(s: [f64; 4]) -> [f64; 4] {
    s.map(|v| f64::from(round_half_up(v)) / 255.0)
}

fn to_f16(v: f64) -> f64 {
    f64::from(f16::from_f32(v as f32).to_f32())
}

fn over_in_f16(s: [f64; 4], d: [f64; 4]) -> [f64; 4] {
    let s = s.map(to_f16);
    let d = d.map(to_f16);
    let keep = to_f16(1.0 - s[3]);
    std::array::from_fn(|i| to_f16(s[i] + to_f16(d[i] * keep)))
}

fn fixed(s: [f64; 4], d8: [u8; 4], divide: fn(u32) -> u32) -> [u8; 4] {
    let s8 = s.map(|v| u32::from(round_half_up(v)));
    std::array::from_fn(|i| (s8[i] + divide(u32::from(d8[i]) * (255 - s8[3]))).min(255) as u8)
}

fn over_f32(s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let keep = 1.0 - s[3];
    std::array::from_fn(|i| s[i] + d[i] * keep)
}

fn over_f32_fma(s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let keep = 1.0 - s[3];
    std::array::from_fn(|i| d[i].mul_add(keep, s[i]))
}

fn over_f32_subtract(s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| s[i] + d[i] - d[i] * s[3])
}

fn unorm_to_f32_divided(d8: [u8; 4]) -> [f32; 4] {
    d8.map(|v| f32::from(v) / 255.0)
}

fn unorm_to_f32_scaled(d8: [u8; 4]) -> [f32; 4] {
    d8.map(|v| f32::from(v) * (1.0 / 255.0))
}

fn store_unorm_half_up(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

fn store_unorm_half_even(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8
}

fn store_unorm_scaled_half_even(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.0).round_ties_even() as u8
}

fn fixed16(
    s: [f64; 4],
    d8: [u8; 4],
    round_source: fn(f64) -> u32,
    finish: fn(u32) -> u8,
) -> [u8; 4] {
    let s16 = s.map(|v| round_source(v.clamp(0.0, 1.0) * 65535.0));
    std::array::from_fn(|i| {
        let d16 = u32::from(d8[i]) * 257;
        let out16 = (s16[i] + (d16 * (65535 - s16[3]) + 32767) / 65535).min(65535);
        finish(out16)
    })
}

fn exact_then_f32(s: [f64; 4], d8: [u8; 4]) -> [f32; 4] {
    over(s, d8.map(|v| f64::from(v) / 255.0)).map(|v| v as f32)
}

fn scaled255(s: [f64; 4], d8: [u8; 4], blend: fn(f32, f32, f32, f32) -> f32) -> [f32; 4] {
    let s = s.map(|v| v as f32);
    let s255 = s.map(|v| v * 255.0);
    std::array::from_fn(|i| blend(s255[i], f32::from(d8[i]), s[3], s255[3]))
}

fn store_scaled_half_even(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round_ties_even() as u8
}

fn store_scaled_half_up(v: f32) -> u8 {
    (v.clamp(0.0, 255.0) + 0.5).floor() as u8
}

fn quantized_in_f32_half_up(s: [f64; 4]) -> [u8; 4] {
    s.map(|v| ((v as f32).clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8)
}

fn quantized_in_f32_half_even(s: [f64; 4]) -> [u8; 4] {
    s.map(|v| ((v as f32).clamp(0.0, 1.0) * 255.0).round_ties_even() as u8)
}

fn blend_quantized_float(s8: [u8; 4], d8: [u8; 4], round: fn(f64) -> u8) -> [u8; 4] {
    over(
        s8.map(|v| f64::from(v) / 255.0),
        d8.map(|v| f64::from(v) / 255.0),
    )
    .map(round)
}

fn blend_quantized_fixed(s8: [u8; 4], d8: [u8; 4], divide: fn(u32) -> u32) -> [u8; 4] {
    std::array::from_fn(|i| {
        (u32::from(s8[i]) + divide(u32::from(d8[i]) * (255 - u32::from(s8[3])))).min(255) as u8
    })
}

type UnormModel = (&'static str, fn([f64; 4], [u8; 4]) -> [u8; 4]);

const UNORM_MODELS: &[UnormModel] = &[
    ("float, round half up", |s, d8| {
        over(s, d8.map(|v| f64::from(v) / 255.0)).map(round_half_up)
    }),
    ("float, round half even", |s, d8| {
        over(s, d8.map(|v| f64::from(v) / 255.0)).map(round_half_even)
    }),
    ("source quantized first, round half up", |s, d8| {
        over(quantized(s), d8.map(|v| f64::from(v) / 255.0)).map(round_half_up)
    }),
    ("source quantized first, round half even", |s, d8| {
        over(quantized(s), d8.map(|v| f64::from(v) / 255.0)).map(round_half_even)
    }),
    ("fixed point, (x + 127) / 255", |s, d8| {
        fixed(s, d8, |x| (x + 127) / 255)
    }),
    ("fixed point, (x + 128) / 255", |s, d8| {
        fixed(s, d8, |x| (x + 128) / 255)
    }),
    ("fixed point, x / 255", |s, d8| fixed(s, d8, |x| x / 255)),
    ("fixed point, (x + 128 + ((x + 128) >> 8)) >> 8", |s, d8| {
        fixed(s, d8, |x| (x + 128 + ((x + 128) >> 8)) >> 8)
    }),
    ("half arithmetic per operation, round half up", |s, d8| {
        over_in_f16(s, d8.map(|v| f64::from(v) / 255.0)).map(round_half_up)
    }),
    ("half arithmetic per operation, round half even", |s, d8| {
        over_in_f16(s, d8.map(|v| f64::from(v) / 255.0)).map(round_half_even)
    }),
    ("half source, float blend, round half up", |s, d8| {
        over(s.map(to_f16), d8.map(|v| f64::from(v) / 255.0)).map(round_half_up)
    }),
    ("f32 s + d*(1-a), d/255, half up", |s, d8| {
        over_f32(s.map(|v| v as f32), unorm_to_f32_divided(d8)).map(store_unorm_half_up)
    }),
    ("f32 s + d*(1-a), d/255, half even", |s, d8| {
        over_f32(s.map(|v| v as f32), unorm_to_f32_divided(d8)).map(store_unorm_half_even)
    }),
    ("f32 s + d*(1-a), d*(1/255), half even", |s, d8| {
        over_f32(s.map(|v| v as f32), unorm_to_f32_scaled(d8)).map(store_unorm_half_even)
    }),
    ("f32 fma(d, 1-a, s), d/255, half even", |s, d8| {
        over_f32_fma(s.map(|v| v as f32), unorm_to_f32_divided(d8)).map(store_unorm_half_even)
    }),
    ("f32 fma(d, 1-a, s), d*(1/255), half even", |s, d8| {
        over_f32_fma(s.map(|v| v as f32), unorm_to_f32_scaled(d8)).map(store_unorm_half_even)
    }),
    ("f32 s + d - d*a, d/255, half even", |s, d8| {
        over_f32_subtract(s.map(|v| v as f32), unorm_to_f32_divided(d8)).map(store_unorm_half_even)
    }),
    ("f32 s + d - d*a, d*(1/255), half even", |s, d8| {
        over_f32_subtract(s.map(|v| v as f32), unorm_to_f32_scaled(d8)).map(store_unorm_half_even)
    }),
    ("f32 fma, d/255, half up", |s, d8| {
        over_f32_fma(s.map(|v| v as f32), unorm_to_f32_divided(d8)).map(store_unorm_half_up)
    }),
    ("f32 s + d*(1-a), d/255, scaled half even", |s, d8| {
        over_f32(s.map(|v| v as f32), unorm_to_f32_divided(d8)).map(store_unorm_scaled_half_even)
    }),
    (
        "exact blend rounded once to f32, then half even",
        |s, d8| exact_then_f32(s, d8).map(store_unorm_half_even),
    ),
    ("exact blend rounded once to f32, then half up", |s, d8| {
        exact_then_f32(s, d8).map(store_unorm_half_up)
    }),
    (
        "exact blend * 255 rounded once to f32, then half even",
        |s, d8| {
            over(s, d8.map(|v| f64::from(v) / 255.0))
                .map(|v| ((v.clamp(0.0, 1.0) * 255.0) as f32).round_ties_even() as u8)
        },
    ),
    ("exact blend, ties down", |s, d8| {
        over(s, d8.map(|v| f64::from(v) / 255.0))
            .map(|v| (v.clamp(0.0, 1.0) * 255.0 - 0.5).ceil() as u8)
    }),
    (
        "unorm16 intermediate, source half even, (x + 128) / 257",
        |s, d8| {
            fixed16(
                s,
                d8,
                |v| v.round_ties_even() as u32,
                |o| ((o + 128) / 257) as u8,
            )
        },
    ),
    (
        "unorm16 intermediate, source half up, (x + 128) / 257",
        |s, d8| {
            fixed16(
                s,
                d8,
                |v| (v + 0.5).floor() as u32,
                |o| ((o + 128) / 257) as u8,
            )
        },
    ),
    ("unorm16 intermediate, source half even, x >> 8", |s, d8| {
        fixed16(s, d8, |v| v.round_ties_even() as u32, |o| (o >> 8) as u8)
    }),
    ("f32 at 255 scale: s255 + d8*(1-a), half even", |s, d8| {
        scaled255(s, d8, |s255, d, a, _| s255 + d * (1.0 - a)).map(store_scaled_half_even)
    }),
    ("f32 at 255 scale: s255 + d8*(1-a), half up", |s, d8| {
        scaled255(s, d8, |s255, d, a, _| s255 + d * (1.0 - a)).map(store_scaled_half_up)
    }),
    (
        "f32 at 255 scale: fma(d8, 1-a, s255), half even",
        |s, d8| {
            scaled255(s, d8, |s255, d, a, _| d.mul_add(1.0 - a, s255)).map(store_scaled_half_even)
        },
    ),
    ("f32 at 255 scale: fma(d8, 1-a, s255), half up", |s, d8| {
        scaled255(s, d8, |s255, d, a, _| d.mul_add(1.0 - a, s255)).map(store_scaled_half_up)
    }),
    (
        "f32 at 255 scale: s255 + d8*(255-a255)/255, half even",
        |s, d8| {
            scaled255(s, d8, |s255, d, _, a255| s255 + d * (255.0 - a255) / 255.0)
                .map(store_scaled_half_even)
        },
    ),
    (
        "f32 at 255 scale: s255 + d8*(255-a255)/255, half up",
        |s, d8| {
            scaled255(s, d8, |s255, d, _, a255| s255 + d * (255.0 - a255) / 255.0)
                .map(store_scaled_half_up)
        },
    ),
    (
        "f32 at 255 scale: (s255*255 + d8*(255-a255))/255, half even",
        |s, d8| {
            scaled255(s, d8, |s255, d, _, a255| {
                (s255 * 255.0 + d * (255.0 - a255)) / 255.0
            })
            .map(store_scaled_half_even)
        },
    ),
    ("f32 at 255 scale: s255 + d8 - d8*a, half even", |s, d8| {
        scaled255(s, d8, |s255, d, a, _| s255 + d - d * a).map(store_scaled_half_even)
    }),
    ("f32 at 255 scale: s255 + d8 - d8*a, half up", |s, d8| {
        scaled255(s, d8, |s255, d, a, _| s255 + d - d * a).map(store_scaled_half_up)
    }),
    (
        "f32 at 255 scale: s255 + fma(-d8, a, d8), half even",
        |s, d8| {
            scaled255(s, d8, |s255, d, a, _| s255 + (-d).mul_add(a, d)).map(store_scaled_half_even)
        },
    ),
    (
        "source quantized in f32 half up, float blend, half up",
        |s, d8| blend_quantized_float(quantized_in_f32_half_up(s), d8, round_half_up),
    ),
    (
        "source quantized in f32 half up, float blend, half even",
        |s, d8| blend_quantized_float(quantized_in_f32_half_up(s), d8, round_half_even),
    ),
    (
        "source quantized in f32 half even, float blend, half up",
        |s, d8| blend_quantized_float(quantized_in_f32_half_even(s), d8, round_half_up),
    ),
    (
        "source quantized in f32 half up, fixed (x + 127) / 255",
        |s, d8| blend_quantized_fixed(quantized_in_f32_half_up(s), d8, |x| (x + 127) / 255),
    ),
    (
        "source quantized in f32 half up, fixed (x + 128) / 255",
        |s, d8| blend_quantized_fixed(quantized_in_f32_half_up(s), d8, |x| (x + 128) / 255),
    ),
    ("source quantized in f32 half up, fixed x / 255", |s, d8| {
        blend_quantized_fixed(quantized_in_f32_half_up(s), d8, |x| x / 255)
    }),
    (
        "source quantized in f32 half up, fixed (x + 128 + ((x + 128) >> 8)) >> 8",
        |s, d8| {
            blend_quantized_fixed(quantized_in_f32_half_up(s), d8, |x| {
                (x + 128 + ((x + 128) >> 8)) >> 8
            })
        },
    ),
    (
        "source quantized in f32 half even, fixed (x + 127) / 255",
        |s, d8| blend_quantized_fixed(quantized_in_f32_half_even(s), d8, |x| (x + 127) / 255),
    ),
];

type HalfModel = (&'static str, fn([f64; 4], [f16; 4]) -> [f16; 4]);

const HALF_MODELS: &[HalfModel] = &[
    ("float blend, one rounding to half", |s, d| {
        over(s, d.map(|v| f64::from(v.to_f32()))).map(|v| f16::from_f32(v as f32))
    }),
    ("half arithmetic per operation", |s, d| {
        over_in_f16(s, d.map(|v| f64::from(v.to_f32()))).map(|v| f16::from_f32(v as f32))
    }),
    ("half source, float blend, one rounding", |s, d| {
        over(s.map(to_f16), d.map(|v| f64::from(v.to_f32()))).map(|v| f16::from_f32(v as f32))
    }),
    ("f32 s + d*(1-a), one rounding", |s, d| {
        over_f32(s.map(|v| v as f32), d.map(f16::to_f32)).map(f16::from_f32)
    }),
    ("f32 fma(d, 1-a, s), one rounding", |s, d| {
        over_f32_fma(s.map(|v| v as f32), d.map(f16::to_f32)).map(f16::from_f32)
    }),
    ("f32 s + d - d*a, one rounding", |s, d| {
        over_f32_subtract(s.map(|v| v as f32), d.map(f16::to_f32)).map(f16::from_f32)
    }),
    ("f32 s + d - d*a, denormals flushed", |s, d| {
        over_f32_subtract(s.map(|v| v as f32), d.map(f16::to_f32)).map(|v| {
            let h = f16::from_f32(v);
            if h.to_bits() & 0x7c00 == 0 {
                f16::from_f32(0.0)
            } else {
                h
            }
        })
    }),
];

fn census<T: PartialEq + std::fmt::Debug + Copy, const N: usize>(
    label: &str,
    observed: impl Fn(usize) -> [T; N],
    models: &[(&'static str, Box<dyn Fn(u32, u32) -> [T; N]>)],
) -> (usize, String) {
    let mut report = format!("{label}\n");
    let mut best = usize::MAX;
    for (name, model) in models {
        let mut mismatches = 0usize;
        let mut examples = Vec::new();
        for index in 0..PIXELS {
            let (x, y) = (index as u32 % SIZE, index as u32 / SIZE);
            let expected = model(x, y);
            let seen = observed(index);
            let wrong = (0..N).filter(|&i| expected[i] != seen[i]).count();
            if wrong > 0 {
                mismatches += wrong;
                if examples.len() < 3 {
                    examples.push(format!("({x},{y}) gpu {seen:?} model {expected:?}"));
                }
                if std::env::var_os("CENSUS_RESIDUE").is_some() && mismatches <= 24 {
                    let s = source(x, y).map(f64::from);
                    let exact = over(s, destination_unorm(x, y).map(|v| f64::from(v) / 255.0));
                    let _ = writeln!(
                        report,
                        "    residue ({x},{y}) gpu {seen:?} model {expected:?} exact*255 {:?}",
                        exact.map(|v| v * 255.0)
                    );
                }
            }
        }
        best = best.min(mismatches);
        let _ = writeln!(
            report,
            "  {mismatches:>7} channel mismatches of {}: {name}{}",
            PIXELS * N,
            if mismatches > 0 {
                format!("  e.g. {}", examples.join("; "))
            } else {
                String::new()
            }
        );
    }
    (best, report)
}

fn unorm_census(gpu: &Gpu) -> (usize, String) {
    let destination: Vec<u8> = (0..PIXELS)
        .flat_map(|i| destination_unorm(i as u32 % SIZE, i as u32 / SIZE))
        .collect();
    let bytes = gpu.blend(wgpu::TextureFormat::Rgba8Unorm, &destination);
    let models: Vec<(&'static str, Box<dyn Fn(u32, u32) -> [u8; 4]>)> = UNORM_MODELS
        .iter()
        .map(|(name, model)| {
            let model = *model;
            let boxed: Box<dyn Fn(u32, u32) -> [u8; 4]> =
                Box::new(move |x, y| model(source(x, y).map(f64::from), destination_unorm(x, y)));
            (*name, boxed)
        })
        .collect();
    census(
        "Rgba8Unorm attachment, premultiplied src-over",
        |i| {
            [
                bytes[i * 4],
                bytes[i * 4 + 1],
                bytes[i * 4 + 2],
                bytes[i * 4 + 3],
            ]
        },
        &models,
    )
}

fn half_census(gpu: &Gpu) -> (usize, String) {
    let destination: Vec<u8> = (0..PIXELS)
        .flat_map(|i| destination_half(i as u32 % SIZE, i as u32 / SIZE))
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let bytes = gpu.blend(wgpu::TextureFormat::Rgba16Float, &destination);
    let models: Vec<(&'static str, Box<dyn Fn(u32, u32) -> [f16; 4]>)> = HALF_MODELS
        .iter()
        .map(|(name, model)| {
            let model = *model;
            let boxed: Box<dyn Fn(u32, u32) -> [f16; 4]> =
                Box::new(move |x, y| model(source(x, y).map(f64::from), destination_half(x, y)));
            (*name, boxed)
        })
        .collect();
    census(
        "Rgba16Float attachment, premultiplied src-over",
        |i| {
            std::array::from_fn(|c| {
                f16::from_le_bytes([bytes[i * 8 + c * 2], bytes[i * 8 + c * 2 + 1]])
            })
        },
        &models,
    )
}

/// The renderer's exactness arguments rest on two attachment identities: a
/// zero premultiplied source leaves the destination byte for byte (the rim
/// draw's discarded fragments, a transparent probe), and an opaque source
/// stores as its own 8-bit conversion (the prefix's copy equals its
/// composite). The census of blend models is printed as evidence: on Apple
/// M5 no shader-side model reproduces src-over at exact ties, which is why a
/// draw is never folded into the one beneath it.
#[test]
fn a_zero_source_leaves_the_attachment_and_an_opaque_source_stores_its_own_conversion() {
    let _lock = support::gpu_test_lock();
    let Some(gpu) = gpu() else {
        eprintln!("skipping blend census: no adapter");
        return;
    };
    let destination: Vec<u8> = (0..PIXELS)
        .flat_map(|i| destination_unorm(i as u32 % SIZE, i as u32 / SIZE))
        .collect();
    let bytes = gpu.blend(wgpu::TextureFormat::Rgba8Unorm, &destination);
    for y in 0..SIZE {
        let i = (y * SIZE) as usize * 4;
        assert_eq!(
            &bytes[i..i + 4],
            &destination[i..i + 4],
            "{}: a zero premultiplied source changed row {y}",
            gpu.name
        );
    }
    let opaque_row = ((SIZE - 1) * SIZE) as usize;
    let stored: Vec<[u8; 4]> = (1..SIZE)
        .map(|x| {
            let i = (opaque_row + x as usize) * 4;
            [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
        })
        .collect();
    let rule = [
        (
            "round half up",
            (|s: [f64; 4]| s.map(round_half_up)) as fn([f64; 4]) -> [u8; 4],
        ),
        ("round half even", |s: [f64; 4]| s.map(round_half_even)),
        ("round half up in f32", quantized_in_f32_half_up),
        ("round half even in f32", quantized_in_f32_half_even),
    ]
    .into_iter()
    .find(|(_, round)| {
        (1..SIZE)
            .zip(&stored)
            .all(|(x, seen)| round(source(x, SIZE - 1).map(f64::from)) == *seen)
    });
    let (unorm_best, unorm_report) = unorm_census(&gpu);
    let (half_best, half_report) = half_census(&gpu);
    eprintln!(
        "[blend-census] {} opaque store: {}; best src-over model misses {unorm_best} \
         (Rgba8Unorm) and {half_best} (Rgba16Float) of {} channels\n{unorm_report}{half_report}",
        gpu.name,
        rule.map_or("no listed rule", |(name, _)| name),
        PIXELS * 4
    );
    assert!(
        rule.is_some(),
        "{}: an opaque source stored as neither round-half-up nor round-half-even of itself: \
         {:?}",
        gpu.name,
        &stored[..8]
    );
}
