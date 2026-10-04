//! The forked wgpu's Vulkan barriers, checked by the Khronos validation
//! layer's synchronization validation.
//!
//! A command buffer begun before a bind group layout lets a vertex shader
//! read a texture waits for texture writes only in fragment and compute.
//! These tests read such a texture from a vertex shader afterwards.
//!
//! `just vulkan-sync` provides the layer, enables its analysis of shader
//! accesses and sets `CRANPOSE_REQUIRE_SYNC_VALIDATION`. Without it, a
//! machine with no Vulkan adapter or no validation layer checks nothing,
//! and so does a process where another test installed the logger first:
//! nextest gives each test its own process.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// What wgpu-hal logged since the running test took the GPU.
static MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static GPU: Mutex<()> = Mutex::new(());

struct HalLog;

impl log::Log for HalLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target().starts_with("wgpu_hal")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            messages().push(record.args().to_string());
        }
        if record.level() <= log::Level::Warn {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static HAL_LOG: HalLog = HalLog;

fn messages() -> MutexGuard<'static, Vec<String>> {
    MESSAGES.lock().unwrap_or_else(PoisonError::into_inner)
}

const SHADER: &str = r"
@group(0) @binding(0) var sampled: texture_2d<f32>;

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

fn corner(index: u32) -> vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@vertex fn vs_plain(@builtin(vertex_index) index: u32) -> Varyings {
    return Varyings(corner(index), vec4<f32>(0.0));
}

@vertex fn vs_reads(@builtin(vertex_index) index: u32) -> Varyings {
    return Varyings(corner(index), textureLoad(sampled, vec2<i32>(0, 0), 0));
}

@fragment fn fs_color(varyings: Varyings) -> @location(0) vec4<f32> {
    return varyings.color;
}

@fragment fn fs_reads(varyings: Varyings) -> @location(0) vec4<f32> {
    return textureLoad(sampled, vec2<i32>(varyings.position.xy), 0);
}
";

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A draw that reads a texture.
struct Reader {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    module: wgpu::ShaderModule,
    _turn: MutexGuard<'static, ()>,
}

impl Gpu {
    /// A Vulkan device under validation, or `None` where this machine
    /// cannot check synchronization and the run does not require it.
    fn request() -> Option<Self> {
        let turn = GPU.lock().unwrap_or_else(PoisonError::into_inner);
        if log::set_logger(&HAL_LOG).is_ok() {
            log::set_max_level(log::LevelFilter::Debug);
        }
        messages().clear();
        let required = std::env::var_os("CRANPOSE_REQUIRE_SYNC_VALIDATION").is_some();

        // `DEBUG` installs the messenger that carries the layer's reports.
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN;
        descriptor.flags = wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
            Ok(adapter) => adapter,
            Err(err) => {
                assert!(!required, "no Vulkan adapter: {err:?}");
                return None;
            }
        };
        let reports = {
            let log = messages();
            log.iter()
                .any(|message| message.contains("Enabling debug utils"))
                && !log
                    .iter()
                    .any(|message| message.contains("unable to find layer"))
        };
        if !reports {
            assert!(
                !required,
                "the Khronos validation layer's reports are missing"
            );
            return None;
        }
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("Vulkan device");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        Some(Self {
            device,
            queue,
            module,
            _turn: turn,
        })
    }

    fn texture(&self, label: &str) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    /// A draw whose `stage` shader reads `texture`; its bind group layout
    /// is created here.
    fn reader(&self, stage: wgpu::ShaderStages, texture: &wgpu::Texture) -> Reader {
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: stage,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let (vertex, fragment) = if stage == wgpu::ShaderStages::VERTEX {
            ("vs_reads", "fs_color")
        } else {
            ("vs_plain", "fs_reads")
        };
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vertex),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.module,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(FORMAT.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &texture.create_view(&Default::default()),
                ),
            }],
        });
        Reader {
            pipeline,
            bind_group,
        }
    }

    fn encoder(&self) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(&Default::default())
    }

    /// Submits `buffers` as one submission and fails on any hazard the
    /// layer reports.
    fn submit(&self, buffers: impl IntoIterator<Item = wgpu::CommandBuffer>) {
        self.queue.submit(buffers);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the submission finishes");
        let hazards: Vec<String> = messages()
            .iter()
            .filter(|message| message.contains("SYNC-HAZARD"))
            .cloned()
            .collect();
        assert!(hazards.is_empty(), "{}", hazards.join("\n\n"));
    }
}

/// A render pass that clears `target` and runs `reader`'s draw, if any.
fn draw(encoder: &mut wgpu::CommandEncoder, target: &wgpu::Texture, reader: Option<&Reader>) {
    let view = target.create_view(&Default::default());
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    if let Some(reader) = reader {
        pass.set_pipeline(&reader.pipeline);
        pass.set_bind_group(0, &reader.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A texture to sample, a target to draw into and a draw that samples the
/// texture in a fragment shader, all made before any vertex reader.
struct Scene {
    gpu: Gpu,
    sampled: wgpu::Texture,
    output: wgpu::Texture,
    fragment_reader: Reader,
}

impl Scene {
    fn request() -> Option<Self> {
        let gpu = Gpu::request()?;
        let sampled = gpu.texture("sampled");
        let output = gpu.texture("output");
        let fragment_reader = gpu.reader(wgpu::ShaderStages::FRAGMENT, &sampled);
        Some(Self {
            gpu,
            sampled,
            output,
            fragment_reader,
        })
    }

    /// A buffer that draws `sampled`, then samples it while drawing
    /// `output`.
    fn draw_then_read(&self) -> wgpu::CommandBuffer {
        let mut encoder = self.gpu.encoder();
        draw(&mut encoder, &self.sampled, None);
        draw(&mut encoder, &self.output, Some(&self.fragment_reader));
        encoder.finish()
    }

    /// A buffer that draws `output` with `reader`'s draw, if any.
    fn draw_output(&self, reader: Option<&Reader>) -> wgpu::CommandBuffer {
        let mut encoder = self.gpu.encoder();
        draw(&mut encoder, &self.output, reader);
        encoder.finish()
    }

    fn vertex_reader(&self) -> Reader {
        self.gpu.reader(wgpu::ShaderStages::VERTEX, &self.sampled)
    }
}

#[test]
fn a_vertex_shader_waits_for_a_texture_drawn_before_its_layout_existed() {
    let Some(scene) = Scene::request() else {
        return;
    };
    let early = scene.draw_then_read();
    let vertex_reader = scene.vertex_reader();
    let late = scene.draw_output(Some(&vertex_reader));
    scene.gpu.submit([early, late]);
}

#[test]
fn a_vertex_shader_waits_for_a_texture_drawn_by_an_older_buffer_submitted_before_it() {
    let Some(scene) = Scene::request() else {
        return;
    };
    let early = scene.draw_then_read();
    let vertex_reader = scene.vertex_reader();
    let first = scene.draw_output(None);
    let second = scene.draw_output(Some(&vertex_reader));
    scene.gpu.submit([first, early, second]);
}

#[test]
fn an_older_buffer_drawing_over_a_texture_waits_for_vertex_reads_submitted_before_it() {
    let Some(scene) = Scene::request() else {
        return;
    };
    scene.gpu.submit([scene.draw_then_read()]);
    let mut early = scene.gpu.encoder();
    draw(&mut early, &scene.output, Some(&scene.fragment_reader));
    draw(&mut early, &scene.sampled, None);
    let early = early.finish();
    let vertex_reader = scene.vertex_reader();
    let late = scene.draw_output(Some(&vertex_reader));
    scene.gpu.submit([late, early]);
}
