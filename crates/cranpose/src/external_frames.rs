use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cranpose_ui::{ExternalTexture, ImageBitmap};

use crate::GpuContext;

/// The most textures one [`ExternalFrames`] holds: the frame a writer
/// prepares, the one it renders, the one it published and the one shown. An
/// engine that prepares a frame while its render thread renders the frame
/// before needs all four.
const MAX_TEXTURES: usize = 4;

/// Frames that another thread renders with Cranpose's device, such as a game
/// engine that runs its own loop, for a composition to show.
///
/// The thread renders through a [`FrameWriter`] into textures that this ring
/// owns and publishes each frame once the queue has its work. The composition
/// takes the newest published frame with [`Self::next_frame`]. Neither side
/// waits for the other: a writer that renders faster than the composition
/// draws loses its older frames, and a slower one leaves the last frame shown.
///
/// A texture goes back to the writer only when the composition shows a newer
/// frame and no bitmap of the texture is left, so no draw of it is still on its
/// way to the queue. Work the writer submits after that runs after every
/// Cranpose frame that sampled the texture.
pub struct ExternalFrames {
    ring: Arc<Mutex<Ring>>,
}

/// The rendering end of [`ExternalFrames`]. Clones write to the same frames
/// and may live on any thread.
#[derive(Clone)]
pub struct FrameWriter {
    ring: Arc<Mutex<Ring>>,
}

/// A texture that a [`FrameWriter`] renders one frame into.
pub struct FrameTexture {
    number: u64,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct Ring {
    gpu: GpuContext,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    opaque: bool,
    size: (u32, u32),
    next: u64,
    slots: Vec<Slot>,
    encoder: Option<SrgbEncoder>,
}

struct Slot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    shown: ExternalTexture,
    state: State,
    // What the encoder reads and writes for this slot, when the ring has one.
    encode: Option<EncodeTarget>,
}

// Cranpose samples a texture as stored, so it reads an sRGB frame through a
// view without the sRGB conversion. A device that cannot view a texture in a
// second format, such as WebGL2, gets a second texture of the format without
// the conversion instead, which this pass fills from the frame when the
// writer publishes it.
#[derive(Clone)]
struct SrgbEncoder {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

#[derive(Clone)]
struct EncodeTarget {
    frame: wgpu::BindGroup,
    shown: wgpu::TextureView,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Writing(u64),
    Ready(u64),
    Shown,
}

impl ExternalFrames {
    /// Frames in textures of `format` that can also be used as `usage`, such
    /// as `RENDER_ATTACHMENT`. `opaque` promises that every pixel the writer
    /// renders has full alpha.
    pub fn new(
        gpu: &GpuContext,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        opaque: bool,
    ) -> Self {
        let views_in_two_formats = gpu
            .adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::VIEW_FORMATS);
        let encoder = (format.is_srgb() && !views_in_two_formats)
            .then(|| SrgbEncoder::new(gpu, format.remove_srgb_suffix()));
        Self {
            ring: Arc::new(Mutex::new(Ring {
                gpu: gpu.clone(),
                format,
                usage,
                opaque,
                size: (0, 0),
                next: 1,
                slots: Vec::with_capacity(MAX_TEXTURES),
                encoder,
            })),
        }
    }

    /// The end that renders the frames.
    pub fn writer(&self) -> FrameWriter {
        FrameWriter {
            ring: Arc::clone(&self.ring),
        }
    }

    /// Asks for frames of `width` by `height` pixels from the next
    /// [`FrameWriter::begin`] on. The frame shown stays until one of the new
    /// size is published.
    pub fn set_size(&self, width: u32, height: u32) {
        lock(&self.ring).size = (width, height);
    }

    /// The newest frame published since the last call, or `None` when there
    /// is none. The bitmap's id is new for every frame.
    pub fn next_frame(&self) -> Option<ImageBitmap> {
        let mut ring = lock(&self.ring);
        let (index, number) = ring
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match slot.state {
                State::Ready(number) => Some((index, number)),
                _ => None,
            })
            .max_by_key(|(_, number)| *number)?;
        for slot in &mut ring.slots {
            if slot.state == State::Shown {
                slot.state = State::Free;
            }
        }
        let slot = &mut ring.slots[index];
        slot.state = State::Shown;
        Some(slot.shown.frame(number))
    }
}

impl FrameWriter {
    /// A texture for the next frame, at the size last asked for. `None` while
    /// no size was asked for, or when every texture is still in use because
    /// the composition has not drawn the frames published so far.
    pub fn begin(&self) -> Option<FrameTexture> {
        let mut ring = lock(&self.ring);
        let (width, height) = ring.size;
        if width == 0 || height == 0 {
            return None;
        }
        ring.slots
            .retain(|slot| !slot.reusable() || slot.size() == (width, height));
        let index = match ring
            .slots
            .iter()
            .position(|slot| slot.reusable() && slot.size() == (width, height))
        {
            Some(index) => index,
            None if ring.slots.len() < MAX_TEXTURES => {
                let slot = ring.new_slot(width, height)?;
                ring.slots.push(slot);
                ring.slots.len() - 1
            }
            None => return None,
        };
        let number = ring.next;
        ring.next += 1;
        let slot = &mut ring.slots[index];
        slot.state = State::Writing(number);
        Some(FrameTexture {
            number,
            texture: slot.texture.clone(),
            view: slot.view.clone(),
        })
    }

    /// Hands frame `number` to the composition. Call it once the work that
    /// renders the frame was submitted to the queue. Published frames that
    /// were not shown yet are dropped, and so are older targets that were
    /// never published.
    pub fn publish(&self, number: u64) {
        let encode = {
            let ring = lock(&self.ring);
            let Some(slot) = ring
                .slots
                .iter()
                .find(|slot| slot.state == State::Writing(number))
            else {
                return;
            };
            ring.encoder.clone().zip(slot.encode.clone())
        };
        if let Some((encoder, target)) = encode {
            encoder.encode(&target);
        }
        let mut ring = lock(&self.ring);
        let Some(index) = ring
            .slots
            .iter()
            .position(|slot| slot.state == State::Writing(number))
        else {
            return;
        };
        for slot in &mut ring.slots {
            if let State::Writing(older) | State::Ready(older) = slot.state
                && older < number
            {
                slot.state = State::Free;
            }
        }
        ring.slots[index].state = State::Ready(number);
    }
}

impl FrameTexture {
    /// The number to [`FrameWriter::publish`] once the frame is rendered.
    pub fn number(&self) -> u64 {
        self.number
    }

    /// The texture to render into.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// A view of the whole texture in its own format.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

impl Ring {
    fn new_slot(&self, width: u32, height: u32) -> Option<Slot> {
        let sampled = self.format.remove_srgb_suffix();
        let view_formats = [sampled];
        let texture = self.new_texture(
            width,
            height,
            self.format,
            self.usage,
            if sampled == self.format || self.encoder.is_some() {
                &[]
            } else {
                &view_formats
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let (shown, encode) = match &self.encoder {
            Some(encoder) => {
                let shown = self
                    .new_texture(
                        width,
                        height,
                        sampled,
                        wgpu::TextureUsages::RENDER_ATTACHMENT,
                        &[],
                    )
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let target = EncodeTarget {
                    frame: encoder.bind(&view),
                    shown: shown.clone(),
                };
                (shown, Some(target))
            }
            None => (
                texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(sampled),
                    ..Default::default()
                }),
                None,
            ),
        };
        Some(Slot {
            view,
            shown: ExternalTexture::new(width, height, self.opaque, shown).ok()?,
            texture,
            state: State::Free,
            encode,
        })
    }

    fn new_texture(
        &self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        view_formats: &[wgpu::TextureFormat],
    ) -> wgpu::Texture {
        self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("external frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats,
        })
    }
}

impl SrgbEncoder {
    fn new(gpu: &GpuContext, target: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("external frame encode"),
            source: wgpu::ShaderSource::Wgsl(include_str!("external_frames_encode.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("external frame encode"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("external frame encode"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("external frame encode"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("cover"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("encode"),
                compilation_options: Default::default(),
                targets: &[Some(target.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            device: (*gpu.device).clone(),
            queue: (*gpu.queue).clone(),
            layout,
            pipeline,
        }
    }

    fn bind(&self, frame: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("external frame encode"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(frame),
            }],
        })
    }

    // Submits the pass after the writer's work, so it reads the frame done.
    fn encode(&self, target: &EncodeTarget) {
        let mut commands = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("external frame encode"),
            });
        {
            let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("external frame encode"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.shown,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &target.frame, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([commands.finish()]);
    }
}

impl Slot {
    fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }

    fn reusable(&self) -> bool {
        self.state == State::Free && !self.shown.bitmaps_alive()
    }
}

fn lock(ring: &Mutex<Ring>) -> MutexGuard<'_, Ring> {
    ring.lock().unwrap_or_else(PoisonError::into_inner)
}
