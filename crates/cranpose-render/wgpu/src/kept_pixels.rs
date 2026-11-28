// The kept outputs of layers their shader alone draws, kept on disk between
// launches beside the pipeline cache: a launch uploads the pixels an earlier
// one drew in place of drawing them again. The file names each output by
// what it draws, so an output whose shader or uniforms changed is drawn
// afresh. An output goes to the file when the frame after its fill shows it
// again, so a layer drawn anew every frame never does. An output no launch
// used for a few launches goes. A frame uploads at most FRAME_UPLOAD_BYTES of
// pixels past its first output. Each upload holds a staging copy of its
// pixels until the GPU is done with the frame. A start that shows many
// outputs at once would hold all of those copies. The outputs past the share
// show their placeholder until a later frame uploads them.

use std::{
    fs::File,
    hash::{Hash, Hasher},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    rc::Rc,
};

use cranpose_core::collections::map::{HashMap, HashSet};
use cranpose_render_common::raster_cache::LayerRasterCacheKey;
use cranpose_ui_graphics::FxHasher;

use crate::{
    frame_graph::{
        FrameCommandStats,
        texture_copies::{ReadBackState, ReadBackStatus, TextureReadBack, read_back_textures},
        texture_format_bytes_per_pixel, upload_texture,
    },
    kept_pixels_file::{Drawn, Head, Indexed, Writer, read_index},
    offscreen::OffscreenTarget,
};

pub(crate) struct KeptPixels {
    // none where the app gives no file
    file: Option<KeptFile>,
}

struct KeptFile {
    path: PathBuf,
    launch: u32,
    // the file as this launch found it, held open so its pixels stay
    // readable after a write replaces it
    earlier: Option<File>,
    // where the pixels of earlier launches lie in `earlier`, by key
    index: HashMap<u64, Indexed>,
    // the keys this launch kept or uploaded, whose pixels stay on disk
    used: HashSet<u64>,
    // the outputs this frame drew, and those the last submitted frame drew,
    // which this frame keeps when it shows them again
    drawn: Vec<u64>,
    last_drawn: Vec<u64>,
    // the device and queue that draw the outputs, from the first kept one
    gpu: Option<(wgpu::Device, wgpu::Queue)>,
    // outputs drawn this frame, to copy once the frame is submitted
    requested: Vec<Requested>,
    // copies of drawn outputs in flight to buffers the CPU reads
    reading: Vec<Reading>,
    // started with the launch's first drawn output
    writer: Option<Writer>,
    // the pixels of this frame's uploads, released at its end: a frame
    // that uploads usually uploads many, and later frames seldom do
    upload: Vec<u8>,
    // the bytes this frame uploaded
    uploaded: u64,
}

struct Requested {
    key: u64,
    target: Rc<OffscreenTarget>,
}

struct Reading {
    head: Head,
    // the bytes of a row of pixels, and of a row in the buffer
    unpadded: u32,
    row: u32,
    buffer: wgpu::Buffer,
    state: ReadBackState,
}

// A layer cache key as the file names it.
pub(crate) fn disk_key(key: &LayerRasterCacheKey) -> u64 {
    let mut hasher = FxHasher::default();
    key.hash(&mut hasher);
    hasher.finish()
}

// The bytes of kept pixels a frame uploads past its first output. With two
// frames in flight, the staging copies of about three of these frames are
// alive at once.
const FRAME_UPLOAD_BYTES: u64 = 4 << 20;

fn format_key(format: wgpu::TextureFormat) -> u64 {
    let mut hasher = FxHasher::default();
    format.hash(&mut hasher);
    hasher.finish()
}

// The bytes of a row of `width` pixels of `format`; none past u32.
fn row_bytes(format: wgpu::TextureFormat, width: u32) -> Option<u32> {
    u32::try_from(texture_format_bytes_per_pixel(format))
        .ok()?
        .checked_mul(width)
}

fn pixel_bytes(format: wgpu::TextureFormat, width: u32, height: u32) -> u64 {
    texture_format_bytes_per_pixel(format) * u64::from(width) * u64::from(height)
}

impl KeptPixels {
    // The index of the pixels the last launches kept, beside the pipeline
    // cache file; nothing where the app gives no file.
    pub(crate) fn load() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let path = crate::pipeline_disk_cache::file_path();
        #[cfg(target_arch = "wasm32")]
        let path: Option<PathBuf> = None;
        let file = path.map(|path| {
            let path = path.with_extension("pixels");
            let (launch, earlier, index) = match File::open(&path).ok().and_then(read_index) {
                Some((launch, earlier, index)) => (launch, Some(earlier), index),
                None => (0, None, HashMap::default()),
            };
            if !index.is_empty() {
                log::info!("[kept-pixels] found {} outputs", index.len());
            }
            KeptFile {
                path,
                launch: launch.wrapping_add(1),
                earlier,
                index,
                used: HashSet::default(),
                drawn: Vec::new(),
                last_drawn: Vec::new(),
                gpu: None,
                requested: Vec::new(),
                reading: Vec::new(),
                writer: None,
                upload: Vec::new(),
                uploaded: 0,
            }
        });
        Self { file }
    }

    // Whether an earlier launch kept the pixels of a `format` target of
    // `size` under `key`.
    pub(crate) fn holds(&self, key: u64, size: (u32, u32), format: wgpu::TextureFormat) -> bool {
        self.file
            .as_ref()
            .and_then(|file| file.index.get(&key))
            .is_some_and(|indexed| {
                let head = indexed.head;
                head.format == format_key(format)
                    && (head.width, head.height) == size
                    && u64::from(head.len) == pixel_bytes(format, head.width, head.height)
            })
    }

    // Whether the output an earlier launch kept under `key` waits for a
    // later frame: this frame uploaded its share already.
    pub(crate) fn upload_waits(&self, key: u64) -> bool {
        self.file.as_ref().is_some_and(|file| {
            file.uploaded > 0
                && file.index.get(&key).is_some_and(|indexed| {
                    file.uploaded + u64::from(indexed.head.len) > FRAME_UPLOAD_BYTES
                })
        })
    }

    // Uploads the pixels an earlier launch kept under `key` into `target`;
    // none when none fit it or the file cannot be read.
    pub(crate) fn upload(
        &mut self,
        queue: &wgpu::Queue,
        key: u64,
        target: &OffscreenTarget,
    ) -> Option<FrameCommandStats> {
        let format = target.texture().format();
        if !self.holds(key, (target.width, target.height), format) {
            return None;
        }
        let file = self.file.as_mut()?;
        let indexed = file.index.get(&key).copied()?;
        let earlier = file.earlier.as_mut()?;
        file.upload.resize(indexed.head.len as usize, 0);
        let read = earlier
            .seek(SeekFrom::Start(indexed.offset))
            .and_then(|_| earlier.read_exact(&mut file.upload));
        if let Err(error) = read {
            log::warn!("[kept-pixels] read {:?}: {error}", file.path);
            file.index.remove(&key);
            return None;
        }
        let stats = upload_texture(
            queue,
            target.texture().as_image_copy(),
            &file.upload,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes(format, target.width)?),
                rows_per_image: Some(target.height),
            },
            target.texture().size(),
        );
        file.used.insert(key);
        file.uploaded += stats.upload_bytes;
        Some(stats)
    }

    // Notes that this frame drew the output named `key`.
    pub(crate) fn drew(&mut self, key: u64) {
        if let Some(file) = self.file.as_mut() {
            file.drawn.push(key);
        }
    }

    // Keeps the output under `key`, which `target` of `device` shows again,
    // when the last submitted frame drew it.
    pub(crate) fn shown(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        key: &LayerRasterCacheKey,
        target: &Rc<OffscreenTarget>,
    ) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if file.last_drawn.is_empty() {
            return;
        }
        let key = disk_key(key);
        let Some(at) = file.last_drawn.iter().position(|drawn| *drawn == key) else {
            return;
        };
        file.last_drawn.swap_remove(at);
        file.used.insert(key);
        file.gpu
            .get_or_insert_with(|| (device.clone(), queue.clone()));
        file.requested.push(Requested {
            key,
            target: Rc::clone(target),
        });
    }

    // Forgets the outputs of a frame that submitted nothing: their targets
    // hold no drawn pixels.
    pub(crate) fn forget_unsubmitted(&mut self) {
        if let Some(file) = self.file.as_mut() {
            file.drawn.clear();
        }
    }

    // Whether outputs wait for a later frame: one that shows them again, or
    // one that collects their copies.
    pub(crate) fn awaits_frame(&self) -> bool {
        self.file
            .as_ref()
            .is_some_and(|file| !file.last_drawn.is_empty() || !file.reading.is_empty())
    }

    // Ends a frame: copies the outputs it drew to buffers the CPU reads, and
    // hands the copies that finished to the file.
    pub(crate) fn end_frame(&mut self) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        std::mem::swap(&mut file.last_drawn, &mut file.drawn);
        file.drawn.clear();
        file.upload = Vec::new();
        file.uploaded = 0;
        let Some((device, queue)) = file.gpu.clone() else {
            return;
        };
        if !file.requested.is_empty() {
            file.read_back(&device, &queue);
        }
        if !file.reading.is_empty() {
            if let Err(error) = device.poll(wgpu::PollType::Poll) {
                log::warn!("[kept-pixels] poll: {error}");
            }
            file.write_finished();
        }
    }
}

impl KeptFile {
    fn read_back(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let launch = self.launch;
        let copies: Vec<(Rc<OffscreenTarget>, Reading)> = self
            .requested
            .drain(..)
            .filter_map(|Requested { key, target }| {
                let reading = Reading::new(device, key, &target, launch)?;
                Some((target, reading))
            })
            .collect();
        read_back_textures(
            device,
            queue,
            copies.iter().map(|(target, reading)| TextureReadBack {
                source: target,
                buffer: &reading.buffer,
                bytes_per_row: reading.row,
                state: reading.state.clone(),
            }),
        );
        self.reading
            .extend(copies.into_iter().map(|(_, reading)| reading));
    }

    fn write_finished(&mut self) {
        let settled = self.reading.extract_if(.., |reading| {
            reading.state.status() != ReadBackStatus::Pending
        });
        let drawn: Vec<Drawn> = settled.filter_map(Reading::into_drawn).collect();
        if drawn.is_empty() {
            return;
        }
        if self.writer.is_none() {
            self.writer = Writer::spawn(self.path.clone(), self.launch);
        }
        if let Some(writer) = &self.writer {
            writer.send(drawn, self.used.clone());
        }
    }
}

impl Reading {
    // A buffer to copy `target`, drawn this launch, into; none when a row
    // of its pixels passes u32.
    fn new(device: &wgpu::Device, key: u64, target: &OffscreenTarget, launch: u32) -> Option<Self> {
        let format = target.texture().format();
        let unpadded = row_bytes(format, target.width)?;
        let row = unpadded.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Kept Pixels"),
            size: u64::from(row) * u64::from(target.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Some(Self {
            head: Head {
                key,
                format: format_key(format),
                width: target.width,
                height: target.height,
                last: launch,
                len: unpadded * target.height,
            },
            unpadded,
            row,
            buffer,
            state: ReadBackState::default(),
        })
    }

    // The pixels the buffer holds, its rows' padding left out; none when
    // the buffer cannot be read.
    fn into_drawn(self) -> Option<Drawn> {
        if self.state.status() != ReadBackStatus::Readable {
            return None;
        }
        let bytes = self.buffer.slice(..).get_mapped_range().ok().map(|view| {
            let used = self.unpadded as usize;
            let mut bytes = Vec::with_capacity(self.head.len as usize);
            for line in view
                .chunks(self.row as usize)
                .take(self.head.height as usize)
            {
                bytes.extend_from_slice(&line[..used]);
            }
            bytes
        });
        self.buffer.unmap();
        Some(Drawn {
            head: self.head,
            bytes: bytes?,
        })
    }
}
