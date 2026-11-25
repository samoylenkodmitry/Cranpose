//! The Windows decoder: a Media Foundation hardware `IMFTransform` backed by D3D11/DXGI on a
//! dedicated worker thread.
//!
//! Hardware MFTs (Intel QuickSync, NVIDIA NVDEC, AMD VCN) run asynchronously through
//! `IMFMediaEventGenerator` and decode into D3D11 NV12/P010 surfaces, which are copied on the GPU
//! into pooled shared NT textures for zero-copy D3D12 rendering. Synchronous software MFTs are also
//! supported as a fallback when no hardware MFT is installed.
#![expect(
    unsafe_code,
    reason = "Media Foundation transforms, buffers, and media types are Win32 COM interfaces"
)]

use std::{
    collections::VecDeque,
    mem::ManuallyDrop,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};
use windows::{
    Win32::{
        Graphics::Direct3D11::ID3D11Texture2D,
        Media::MediaFoundation::{
            IMF2DBuffer, IMFDXGIBuffer, IMFMediaBuffer, IMFMediaEventGenerator, IMFSample,
            IMFTransform, METransformDrainComplete, METransformHaveOutput, METransformNeedInput,
            MF_E_NO_EVENTS_AVAILABLE, MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT,
            MF_E_TRANSFORM_STREAM_CHANGE, MF_EVENT_FLAG_NO_WAIT, MFT_MESSAGE_COMMAND_DRAIN,
            MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_DATA_BUFFER_FORMAT_CHANGE,
        },
    },
    core::Interface,
};

use super::{
    AccessUnit, ColorPrimaries, ColorRange, ColorTransfer, DecodeOrder, DecoderConfig,
    DecoderError, DecoderOutput, VideoCodec, VideoColorInfo, VideoFrame, VideoRotation,
    admission::Admission,
    annex_b::AccessUnitNals,
    d3d11_frame::{DxgiPixelFormat, LivePictures, SharedSurfacePool},
    frame_queue::{FrameQueue, Frames},
    media_foundation_capabilities::MfRuntimeGuard,
    media_foundation_transform::{
        OutputFormat, attach_d3d_manager, clamp_crop, configure_input_type, configure_low_latency,
        configure_output_type, create_decoder_mft, create_input_sample, create_output_sample,
        query_output_stream_info, unlock_async_if_needed,
    },
    pending_units::{PendingUnit, PendingUnits},
    sps::StreamMetadata,
};

// Maximum access units queued for the decoder worker thread before backpressure fires.
const MAX_QUEUED_UNITS: usize = 8;
// Polling slice for async MFT event checks when waiting for input/output.
const ASYNC_POLL_WAIT: Duration = Duration::from_millis(2);
// Maximum time `drain` waits for the worker to finish draining the MFT.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

struct QueuedUnit {
    data: Vec<u8>,
    pts_us: u64,
    timestamp: Duration,
    metadata: u64,
    submitted: Instant,
}

enum Command {
    Unit(QueuedUnit),
    Drain(Arc<Completion>),
    Flush(Arc<Completion>),
}

#[derive(Default)]
struct Completion {
    done: Mutex<Option<Result<(), DecoderError>>>,
    ready: Condvar,
}

impl Completion {
    fn finish(&self, result: Result<(), DecoderError>) {
        *self.done.lock() = Some(result);
        self.ready.notify_all();
    }

    fn wait(&self, timeout: Duration) -> Result<(), DecoderError> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.done.lock();
        while guard.is_none() {
            if self.ready.wait_until(&mut guard, deadline).timed_out() {
                return Err(DecoderError::Failed("decoder command timed out".into()));
            }
        }
        guard.take().unwrap_or(Ok(()))
    }
}

struct Mailbox {
    commands: VecDeque<Command>,
    stopped: bool,
}

struct Shared {
    mailbox: Mutex<Mailbox>,
    wake_worker: Condvar,
    wake_feeder: Condvar,
    queue: Arc<FrameQueue>,
    live: LivePictures,
    depth: usize,
    order: DecodeOrder,
    rotation: VideoRotation,
    config_color: Option<VideoColorInfo>,
    stream_meta: Mutex<StreamMetadata>,
    stop: AtomicBool,
}

pub(super) struct Backend {
    shared: Arc<Shared>,
    codec: VideoCodec,
    nals: AccessUnitNals,
    admission: Admission,
    input_wait: Duration,
    _worker: JoinHandle<()>,
}

impl Backend {
    pub(super) fn new(config: DecoderConfig) -> Result<Self, DecoderError> {
        let DecoderConfig {
            codec,
            width,
            height,
            output,
            input_wait,
            order,
            rotation,
            color,
        } = config;
        let DecoderOutput::Frames { depth } = output else {
            return Err(DecoderError::Unavailable);
        };
        let depth = depth.max(2);
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox {
                commands: VecDeque::with_capacity(MAX_QUEUED_UNITS + 2),
                stopped: false,
            }),
            wake_worker: Condvar::new(),
            wake_feeder: Condvar::new(),
            queue: Arc::new(FrameQueue::new(depth, 0)),
            live: LivePictures::default(),
            depth,
            order,
            rotation,
            config_color: color,
            stream_meta: Mutex::new(StreamMetadata::default()),
            stop: AtomicBool::new(false),
        });
        let init_done = Arc::new(Completion::default());
        let worker_shared = Arc::clone(&shared);
        let worker_init = Arc::clone(&init_done);
        let worker = thread::Builder::new()
            .name("video-decoder-mf".into())
            .spawn(move || {
                run_worker(codec, (width, height), order, worker_shared, worker_init);
            })
            .map_err(|error| DecoderError::Failed(format!("decoder thread: {error}")))?;
        init_done.wait(DRAIN_TIMEOUT)?;
        Ok(Self {
            shared,
            codec,
            nals: AccessUnitNals::default(),
            admission: Admission::new((width as usize) * (height as usize), order),
            input_wait,
            _worker: worker,
        })
    }

    pub(super) fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        let pts_us = self.admission.check(&unit)?;
        if self.codec != VideoCodec::Av1 {
            self.nals.parse(self.codec, unit.data);
            if let Some(sps) = self.nals.sequence_parameter_set(unit.data)
                && let Some(meta) = StreamMetadata::parse_sps(self.codec, sps)
            {
                *self.shared.stream_meta.lock() = meta;
            }
        }
        let queued = QueuedUnit {
            data: unit.data.to_vec(),
            pts_us,
            timestamp: unit.timestamp,
            metadata: unit.metadata,
            submitted: Instant::now(),
        };
        let deadline = Instant::now() + self.input_wait;
        let mut mailbox = self.shared.mailbox.lock();
        while !mailbox.stopped && mailbox.commands.len() >= MAX_QUEUED_UNITS {
            if self
                .shared
                .wake_feeder
                .wait_until(&mut mailbox, deadline)
                .timed_out()
            {
                self.shared.queue.stats.refused();
                return Err(DecoderError::Full);
            }
        }
        if mailbox.stopped {
            return Err(DecoderError::Failed("decoder worker stopped".into()));
        }
        mailbox.commands.push_back(Command::Unit(queued));
        drop(mailbox);
        self.shared.wake_worker.notify_one();
        self.admission.accept(pts_us);
        self.shared.queue.stats.submitted();
        Ok(())
    }

    pub(super) fn drain(&mut self) -> Result<(), DecoderError> {
        let completion = Arc::new(Completion::default());
        {
            let mut mailbox = self.shared.mailbox.lock();
            if mailbox.stopped {
                return Err(DecoderError::Failed("decoder worker stopped".into()));
            }
            mailbox
                .commands
                .push_back(Command::Drain(Arc::clone(&completion)));
        }
        self.shared.wake_worker.notify_one();
        completion.wait(DRAIN_TIMEOUT)?;
        self.admission.reset();
        Ok(())
    }

    pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
        let completion = Arc::new(Completion::default());
        {
            let mut mailbox = self.shared.mailbox.lock();
            if mailbox.stopped {
                return Err(DecoderError::Failed("decoder worker stopped".into()));
            }
            mailbox
                .commands
                .retain(|cmd| !matches!(cmd, Command::Unit(_)));
            mailbox
                .commands
                .push_back(Command::Flush(Arc::clone(&completion)));
        }
        self.shared.wake_worker.notify_one();
        completion.wait(DRAIN_TIMEOUT)?;
        self.admission.reset();
        Ok(())
    }

    pub(super) fn frames(&self) -> Frames {
        Frames(Arc::clone(&self.shared.queue))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.mailbox.lock().stopped = true;
        self.shared.wake_worker.notify_all();
        self.shared.wake_feeder.notify_all();
    }
}

struct MftSession {
    _runtime: MfRuntimeGuard,
    pool: SharedSurfacePool,
    transform: IMFTransform,
    async_events: Option<IMFMediaEventGenerator>,
    mft_provides_samples: bool,
    output_sample_size: u32,
    output_format: OutputFormat,
    pending: PendingUnits,
    shared: Arc<Shared>,
    needed_inputs: usize,
    ready_outputs: usize,
}

fn run_worker(
    codec: VideoCodec,
    size: (u32, u32),
    order: DecodeOrder,
    shared: Arc<Shared>,
    init_done: Arc<Completion>,
) {
    let mut session = match MftSession::new(codec, size, order, Arc::clone(&shared)) {
        Ok(session) => {
            init_done.finish(Ok(()));
            session
        }
        Err(error) => {
            shared.mailbox.lock().stopped = true;
            shared.wake_feeder.notify_all();
            init_done.finish(Err(error));
            return;
        }
    };
    session.run_loop();
    shared.mailbox.lock().stopped = true;
    shared.wake_feeder.notify_all();
}

impl MftSession {
    fn new(
        codec: VideoCodec,
        size: (u32, u32),
        order: DecodeOrder,
        shared: Arc<Shared>,
    ) -> Result<Self, DecoderError> {
        let runtime = MfRuntimeGuard::enter().ok_or(DecoderError::Unavailable)?;
        let (pool, device_manager) = SharedSurfacePool::new()?;
        let transform = create_decoder_mft(codec)?;
        let async_events = unlock_async_if_needed(&transform)?;
        configure_low_latency(&transform, order);
        attach_d3d_manager(&transform, &device_manager);
        configure_input_type(&transform, codec, size)?;
        let output_format = configure_output_type(&transform, size)?;
        let (mft_provides_samples, output_sample_size) = query_output_stream_info(&transform)?;
        // SAFETY: begins streaming on the configured MFT.
        unsafe {
            let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }
        Ok(Self {
            _runtime: runtime,
            pool,
            transform,
            async_events,
            mft_provides_samples,
            output_sample_size,
            output_format,
            pending: PendingUnits::new(),
            shared,
            needed_inputs: 0,
            ready_outputs: 0,
        })
    }

    fn run_loop(&mut self) {
        while !self.shared.stop.load(Ordering::Acquire) {
            self.poll_async_events();
            while self.ready_outputs > 0 {
                self.ready_outputs -= 1;
                if self.pull_output().is_err() {
                    return;
                }
            }
            let Some(command) = self.next_command() else {
                break;
            };
            match command {
                Command::Unit(unit) => {
                    if let Err(error) = self.process_unit(unit) {
                        log::warn!("windows video decoder: {error}");
                    }
                }
                Command::Drain(completion) => {
                    let result = self.drain_mft();
                    completion.finish(result);
                }
                Command::Flush(completion) => {
                    let result = self.flush_mft();
                    completion.finish(result);
                }
            }
        }
    }

    fn next_command(&mut self) -> Option<Command> {
        let mut mailbox = self.shared.mailbox.lock();
        loop {
            if self.shared.stop.load(Ordering::Acquire) || mailbox.stopped {
                return None;
            }
            if let Some(cmd) = mailbox.commands.pop_front() {
                self.shared.wake_feeder.notify_one();
                return Some(cmd);
            }
            if self.async_events.is_some() {
                self.shared
                    .wake_worker
                    .wait_for(&mut mailbox, ASYNC_POLL_WAIT);
                drop(mailbox);
                self.poll_async_events();
                while self.ready_outputs > 0 {
                    self.ready_outputs -= 1;
                    let _ = self.pull_output();
                }
                mailbox = self.shared.mailbox.lock();
            } else {
                self.shared.wake_worker.wait(&mut mailbox);
            }
        }
    }

    fn process_unit(&mut self, unit: QueuedUnit) -> Result<(), DecoderError> {
        let sample = create_input_sample(&unit.data, unit.pts_us)?;
        self.pending.push(PendingUnit {
            pts_us: unit.pts_us,
            timestamp: unit.timestamp,
            metadata: unit.metadata,
            submitted: unit.submitted,
        });
        if self.async_events.is_some() {
            self.wait_for_async_input_slot()?;
            // SAFETY: `METransformNeedInput` authorized one `ProcessInput` call.
            unsafe { self.transform.ProcessInput(0, &sample, 0) }
                .map_err(|error| DecoderError::Failed(format!("ProcessInput: {error}")))?;
            self.poll_async_events();
            while self.ready_outputs > 0 {
                self.ready_outputs -= 1;
                self.pull_output()?;
            }
            return Ok(());
        }
        // Synchronous MFT: drain outputs if `MF_E_NOTACCEPTING`, then submit input and drain.
        loop {
            // SAFETY: submits one Annex-B / OBU sample to stream 0.
            match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
                Ok(()) => break,
                Err(error) if error.code() == MF_E_NOTACCEPTING => {
                    if !self.pull_output()? {
                        return Err(DecoderError::Failed(format!("ProcessInput: {error}")));
                    }
                }
                Err(error) => {
                    return Err(DecoderError::Failed(format!("ProcessInput: {error}")));
                }
            }
        }
        while self.pull_output()? {}
        Ok(())
    }

    fn drain_mft(&mut self) -> Result<(), DecoderError> {
        // SAFETY: instructs the MFT to emit all reordered pictures before ending the segment.
        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }
            .map_err(|error| DecoderError::Failed(format!("MFT drain: {error}")))?;
        if self.async_events.is_some() {
            let deadline = Instant::now() + DRAIN_TIMEOUT;
            while !self.shared.stop.load(Ordering::Acquire) && Instant::now() < deadline {
                if self.poll_async_events_until_drain() {
                    break;
                }
                while self.ready_outputs > 0 {
                    self.ready_outputs -= 1;
                    self.pull_output()?;
                }
                self.wait_briefly();
            }
        } else {
            while self.pull_output()? {}
        }
        // SAFETY: restart streaming cleanly after drain so a following segment can decode.
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }
        Ok(())
    }

    fn flush_mft(&mut self) -> Result<(), DecoderError> {
        // SAFETY: flushes buffered input and reordered output inside the MFT.
        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) }
            .map_err(|error| DecoderError::Failed(format!("MFT flush: {error}")))?;
        self.pending.clear();
        self.shared.queue.frames.lock().clear();
        self.needed_inputs = 0;
        self.ready_outputs = 0;
        self.poll_async_events();
        Ok(())
    }

    fn wait_for_async_input_slot(&mut self) -> Result<(), DecoderError> {
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        while self.needed_inputs == 0 {
            if self.shared.stop.load(Ordering::Acquire) {
                return Err(DecoderError::Failed("decoder stopped".into()));
            }
            self.poll_async_events();
            while self.ready_outputs > 0 {
                self.ready_outputs -= 1;
                self.pull_output()?;
            }
            if self.needed_inputs > 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(DecoderError::Full);
            }
            self.wait_briefly();
        }
        self.needed_inputs -= 1;
        Ok(())
    }

    fn wait_briefly(&self) {
        let mut mailbox = self.shared.mailbox.lock();
        if !self.shared.stop.load(Ordering::Acquire) {
            self.shared
                .wake_worker
                .wait_for(&mut mailbox, ASYNC_POLL_WAIT);
        }
    }

    fn poll_async_events(&mut self) {
        let _ = self.poll_async_events_until_drain();
    }

    fn poll_async_events_until_drain(&mut self) -> bool {
        let Some(generator) = self.async_events.as_ref() else {
            return false;
        };
        let mut drained = false;
        loop {
            // SAFETY: non-blocking dequeue of pending async MFT events.
            let event = match unsafe { generator.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => event,
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => break,
                Err(_) => break,
            };
            // SAFETY: reads the `MF_EVENT_TYPE` discriminant of the dequeued event.
            let Ok(kind) = (unsafe { event.GetType() }) else {
                continue;
            };
            if kind == u32::try_from(METransformNeedInput.0).unwrap_or(0) {
                self.needed_inputs += 1;
            } else if kind == u32::try_from(METransformHaveOutput.0).unwrap_or(0) {
                self.ready_outputs += 1;
            } else if kind == u32::try_from(METransformDrainComplete.0).unwrap_or(0) {
                drained = true;
            }
        }
        drained
    }

    fn pull_output(&mut self) -> Result<bool, DecoderError> {
        let sample = if self.mft_provides_samples {
            None
        } else {
            Some(create_output_sample(self.output_sample_size)?)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0u32;
        // SAFETY: calls `ProcessOutput` on stream 0; both `ManuallyDrop` fields are taken below.
        let result = unsafe {
            self.transform
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
        };
        let sample = ManuallyDrop::into_inner(output.pSample);
        let _events = ManuallyDrop::into_inner(output.pEvents);

        let format_changed = (output.dwStatus
            & u32::try_from(MFT_OUTPUT_DATA_BUFFER_FORMAT_CHANGE.0).unwrap_or(0))
            != 0;
        match result {
            Ok(()) => {
                if format_changed {
                    self.refresh_output_type()?;
                }
                if let Some(sample) = sample {
                    self.emit_sample(&sample)?;
                }
                Ok(true)
            }
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                self.refresh_output_type()?;
                Ok(true)
            }
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(false),
            Err(error) => Err(DecoderError::Failed(format!("ProcessOutput: {error}"))),
        }
    }

    fn refresh_output_type(&mut self) -> Result<(), DecoderError> {
        self.output_format = configure_output_type(&self.transform, self.output_format.size)?;
        let (provides, size) = query_output_stream_info(&self.transform)?;
        self.mft_provides_samples = provides;
        self.output_sample_size = size;
        Ok(())
    }

    fn emit_sample(&mut self, sample: &IMFSample) -> Result<(), DecoderError> {
        // SAFETY: reads presentation time in 100-ns ticks (defaults to 0 if unset).
        let pts_100ns = unsafe { sample.GetSampleTime() }.unwrap_or(0).max(0);
        let pts_us = u64::try_from(pts_100ns / 10).unwrap_or(0);
        let unit = match self.shared.order {
            DecodeOrder::LowLatency => self.pending.take(pts_us),
            DecodeOrder::Presentation => self.pending.remove(pts_us),
        };
        let info = PendingUnit::into_frame_info(unit, pts_us);
        // SAFETY: retrieves the single contiguous buffer holding the decoded video frame.
        let buffer: IMFMediaBuffer = unsafe { sample.GetBufferByIndex(0) }
            .map_err(|error| DecoderError::Failed(format!("GetBufferByIndex: {error}")))?;
        let (image, size) = self.import_buffer(&buffer)?;
        let crop = clamp_crop(self.output_format.crop, size);
        let color = self.resolve_color(crop.height.max(size.1));
        let frame = VideoFrame::new(info, size, crop, color, self.shared.rotation, image);
        self.enqueue(frame);
        self.shared.queue.stats.decoded(info.decode_time);
        Ok(())
    }

    fn import_buffer(
        &mut self,
        buffer: &IMFMediaBuffer,
    ) -> Result<(super::d3d11_frame::Image, (u32, u32)), DecoderError> {
        if let Ok(dxgi) = buffer.cast::<IMFDXGIBuffer>() {
            let mut raw = ptr::null_mut();
            // SAFETY: retrieves the underlying `ID3D11Texture2D` from the DXGI media buffer.
            unsafe { dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw) }.map_err(|error| {
                DecoderError::Failed(format!("IMFDXGIBuffer::GetResource: {error}"))
            })?;
            // SAFETY: `GetResource` returned a valid owned `ID3D11Texture2D` reference.
            let texture = unsafe { ID3D11Texture2D::from_raw(raw) };
            // SAFETY: reads the texture array slice index of this decoded picture.
            let subresource = unsafe { dxgi.GetSubresourceIndex() }
                .map_err(|error| DecoderError::Failed(format!("GetSubresourceIndex: {error}")))?;
            return self
                .pool
                .copy_gpu_texture(&texture, subresource, &self.shared.live);
        }
        // Fallback for synchronous software MFTs delivering CPU-locked NV12/P010 memory.
        let size = self.output_format.size;
        let format = self.output_format.pixel_format;
        if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
            let mut scanline0 = ptr::null_mut();
            let mut pitch = 0i32;
            // SAFETY: locks the 2D buffer for reading and unlocks before returning.
            unsafe { buf2d.Lock2D(&mut scanline0, &mut pitch) }
                .map_err(|error| DecoderError::Failed(format!("IMF2DBuffer::Lock2D: {error}")))?;
            let stride = pitch.unsigned_abs();
            let res =
                self.pool
                    .upload_cpu_buffer(scanline0, stride, size, format, &self.shared.live);
            // SAFETY: balances `Lock2D`.
            let _ = unsafe { buf2d.Unlock2D() };
            return res.map(|image| (image, size));
        }
        let mut data = ptr::null_mut();
        // SAFETY: locks linear buffer memory and unlocks immediately after upload.
        unsafe { buffer.Lock(&mut data, None, None) }
            .map_err(|error| DecoderError::Failed(format!("IMFMediaBuffer::Lock: {error}")))?;
        let bytes_per_sample = match format {
            DxgiPixelFormat::Nv12 => 1,
            DxgiPixelFormat::P010 => 2,
        };
        let res = self.pool.upload_cpu_buffer(
            data,
            size.0 * bytes_per_sample,
            size,
            format,
            &self.shared.live,
        );
        // SAFETY: balances `Lock`.
        let _ = unsafe { buffer.Unlock() };
        res.map(|image| (image, size))
    }

    fn resolve_color(&self, height: u32) -> VideoColorInfo {
        let stream = *self.shared.stream_meta.lock();
        let primaries = self
            .output_format
            .primaries
            .or(stream.primaries)
            .or_else(|| self.shared.config_color.map(|c| c.primaries))
            .unwrap_or_else(|| ColorPrimaries::default_for_height(height));
        let range = self
            .output_format
            .range
            .or(stream.range)
            .or_else(|| self.shared.config_color.map(|c| c.range))
            .unwrap_or(ColorRange::Limited);
        let transfer = self
            .output_format
            .transfer
            .or(stream.transfer)
            .or_else(|| self.shared.config_color.map(|c| c.transfer))
            .unwrap_or(ColorTransfer::Sdr);
        VideoColorInfo {
            primaries,
            range,
            transfer,
        }
    }

    fn enqueue(&self, frame: VideoFrame) {
        let mut frames = self.shared.queue.frames.lock();
        while self.shared.live.load(Ordering::Relaxed) > self.shared.depth {
            self.shared.queue.stats.dropped();
            if frames.pop_front().is_none() {
                return;
            }
        }
        frames.push_back(frame);
        drop(frames);
        self.shared.queue.wake();
    }
}
