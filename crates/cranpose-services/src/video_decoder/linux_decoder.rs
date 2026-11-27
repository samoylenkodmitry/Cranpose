//! The Linux decoder: hardware VA-API -> DMABUF via dynamically loaded `libavcodec` + `libva`,
//! with explicit software bi-planar YUV fallback when VA-API hardware decode is absent.
//!
//! Runs on a dedicated worker thread so the UI thread never blocks. Works identically under X11,
//! Wayland, and headless Vulkan because VA-API opens the DRM render node directly.
#![expect(
    unsafe_code,
    reason = "libavcodec, libavutil, and libva calls cross a C ABI boundary"
)]

use std::{
    ffi::{c_int, c_void},
    ptr,
    sync::Arc,
};

use parking_lot::Mutex;

use super::{
    CropRect, DecodeOrder, DecoderConfig, DecoderError, VideoCodec, VideoFrame,
    linux_frame::{
        AvFramePin, DmaBufFrame, DmaBufPlane, Image, LinuxPixelFormat, LivePictures, SoftwarePool,
        take_software_slot,
    },
    pending_units::{PendingUnit, PendingUnits},
    vaapi_capabilities::{av_decoder_names, probe_vaapi_codec},
    vaapi_loader::{
        AV_CODEC_FLAG_LOW_DELAY, AV_PIX_FMT_NONE, AVERROR_EOF, AvApi, AvBufferRef, AvFrameHead,
        AvHwDeviceContext, AvPacketHead, VA_EXPORT_SURFACE_READ_ONLY,
        VA_EXPORT_SURFACE_SEPARATE_LAYERS, VA_FOURCC_P010, VA_STATUS_SUCCESS,
        VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2, VaApi, VaDisplay, VaDrmPrimeSurfaceDescriptor,
        VaSurfaceId, av_api, render_nodes, select_vaapi_format, va_api,
    },
    worker_mailbox::{Command, QueuedUnit, ThreadedBackend, WorkerShared},
};

pub(super) fn open(config: DecoderConfig) -> Result<ThreadedBackend, DecoderError> {
    ThreadedBackend::spawn(
        config,
        "video-decoder-linux",
        |codec, _size, order, shared| LinuxSession::new(codec, order, shared),
        LinuxSession::run_loop,
    )
}

struct LinuxSession {
    av: &'static AvApi,
    va: Option<&'static VaApi>,
    ctx: *mut c_void,
    hw_device_ctx: *mut c_void,
    va_display: VaDisplay,
    packet: *mut c_void,
    frame: *mut c_void,
    software_pool: SoftwarePool,
    pending: PendingUnits,
    shared: Arc<WorkerShared>,
}

impl Drop for LinuxSession {
    fn drop(&mut self) {
        // SAFETY: releases all owned FFmpeg objects allocated in `LinuxSession::new`.
        unsafe {
            (self.av.av_frame_free)(&mut self.frame);
            (self.av.av_packet_free)(&mut self.packet);
            (self.av.avcodec_free_context)(&mut self.ctx);
            if !self.hw_device_ctx.is_null() {
                (self.av.av_buffer_unref)(&mut self.hw_device_ctx);
            }
        }
    }
}

impl LinuxSession {
    fn new(
        codec: VideoCodec,
        order: DecodeOrder,
        shared: Arc<WorkerShared>,
    ) -> Result<Self, DecoderError> {
        let av = av_api().ok_or(DecoderError::Unavailable)?;
        let decoder = find_decoder(av, codec).ok_or(DecoderError::NoDecoder(codec))?;
        // SAFETY: allocates codec context, packet, and reusable receive frame.
        let ctx = unsafe { (av.avcodec_alloc_context3)(decoder) };
        if ctx.is_null() {
            return Err(DecoderError::Failed("avcodec_alloc_context3 failed".into()));
        }
        let packet = unsafe { (av.av_packet_alloc)() };
        let frame = unsafe { (av.av_frame_alloc)() };
        if packet.is_null() || frame.is_null() {
            unsafe {
                let mut f = frame;
                let mut p = packet;
                let mut c = ctx;
                (av.av_frame_free)(&mut f);
                (av.av_packet_free)(&mut p);
                (av.avcodec_free_context)(&mut c);
            }
            return Err(DecoderError::Failed(
                "packet/frame allocation failed".into(),
            ));
        }
        let (va, hw_device_ctx, va_display) = try_attach_vaapi(av, ctx, codec, shared.depth);
        configure_codec_options(av, ctx, order, !hw_device_ctx.is_null());
        // SAFETY: opens the configured libavcodec decoder.
        let opened = unsafe { (av.avcodec_open2)(ctx, decoder, ptr::null_mut()) };
        if opened < 0 {
            let session = Self {
                av,
                va,
                ctx,
                hw_device_ctx,
                va_display,
                packet,
                frame,
                software_pool: Arc::new(Mutex::new(Vec::new())),
                pending: PendingUnits::new(),
                shared,
            };
            drop(session);
            return Err(DecoderError::Configure(format!("avcodec_open2: {opened}")));
        }
        Ok(Self {
            av,
            va,
            ctx,
            hw_device_ctx,
            va_display,
            packet,
            frame,
            software_pool: Arc::new(Mutex::new(Vec::new())),
            pending: PendingUnits::new(),
            shared,
        })
    }

    fn run_loop(&mut self) {
        let shared = Arc::clone(&self.shared);
        while let Some(command) = shared.next_command_with(None, || {}) {
            match command {
                Command::Unit(unit) => {
                    if let Err(error) = self.decode_unit(unit) {
                        log::warn!("linux video decoder: {error}");
                    }
                }
                Command::Drain(completion) => {
                    let result = self.drain_decoder();
                    completion.finish(result);
                }
                Command::Flush(completion) => {
                    self.flush_decoder();
                    completion.finish(Ok(()));
                }
            }
        }
    }

    fn decode_unit(&mut self, unit: QueuedUnit) -> Result<(), DecoderError> {
        let len =
            c_int::try_from(unit.data.len()).map_err(|_| DecoderError::AccessUnitTooLarge {
                size: unit.data.len(),
                limit: c_int::MAX as usize,
            })?;
        // SAFETY: populates the reusable `AVPacket` with `unit.data` and `pts_us`.
        unsafe {
            (self.av.av_packet_unref)(self.packet);
            let alloc = (self.av.av_new_packet)(self.packet, len);
            if alloc < 0 {
                return Err(DecoderError::Failed(format!("av_new_packet: {alloc}")));
            }
            let head = &mut *self.packet.cast::<AvPacketHead>();
            ptr::copy_nonoverlapping(unit.data.as_ptr(), head.data, unit.data.len());
            let pts = i64::try_from(unit.pts_us).unwrap_or(i64::MAX);
            head.pts = pts;
            head.dts = pts;
        }
        self.pending.push(PendingUnit {
            pts_us: unit.pts_us,
            timestamp: unit.timestamp,
            metadata: unit.metadata,
            submitted: unit.submitted,
        });
        // SAFETY: submits the packet to `libavcodec`.
        let send = unsafe { (self.av.avcodec_send_packet)(self.ctx, self.packet) };
        unsafe { (self.av.av_packet_unref)(self.packet) };
        if send < 0 && send != -libc::EAGAIN {
            return Err(DecoderError::Failed(format!("avcodec_send_packet: {send}")));
        }
        self.receive_available_frames()
    }

    fn drain_decoder(&mut self) -> Result<(), DecoderError> {
        // SAFETY: sends NULL packet to signal EOF and flush reordered frames from the DPB.
        let send = unsafe { (self.av.avcodec_send_packet)(self.ctx, ptr::null()) };
        if send < 0 && send != AVERROR_EOF {
            return Err(DecoderError::Failed(format!("avcodec drain: {send}")));
        }
        let result = self.receive_available_frames();
        // SAFETY: resets the decoder after EOF so another segment can start cleanly.
        unsafe { (self.av.avcodec_flush_buffers)(self.ctx) };
        self.pending.clear();
        result
    }

    fn flush_decoder(&mut self) {
        // SAFETY: discards buffered packets and DPB frames inside `libavcodec`.
        unsafe { (self.av.avcodec_flush_buffers)(self.ctx) };
        self.pending.clear();
        self.shared.queue.frames.lock().clear();
    }

    fn receive_available_frames(&mut self) -> Result<(), DecoderError> {
        loop {
            // SAFETY: pulls the next decoded frame into `self.frame`.
            let ret = unsafe { (self.av.avcodec_receive_frame)(self.ctx, self.frame) };
            if ret == -libc::EAGAIN || ret == AVERROR_EOF {
                return Ok(());
            }
            if ret < 0 {
                return Err(DecoderError::Failed(format!(
                    "avcodec_receive_frame: {ret}"
                )));
            }
            self.emit_current_frame()?;
            // SAFETY: releases references held by the scratch receive frame.
            unsafe { (self.av.av_frame_unref)(self.frame) };
        }
    }

    fn emit_current_frame(&mut self) -> Result<(), DecoderError> {
        // SAFETY: reads ABI-stable head fields and runtime-probed `pts` from `self.frame`.
        let (width, height, format, pts_raw) = unsafe {
            let head = &*self.frame.cast::<AvFrameHead>();
            let pts_ptr = self
                .frame
                .cast::<u8>()
                .add(self.av.offsets.frame_pts)
                .cast::<i64>();
            (
                u32::try_from(head.width).unwrap_or(0),
                u32::try_from(head.height).unwrap_or(0),
                head.format,
                *pts_ptr,
            )
        };
        if width == 0 || height == 0 {
            return Ok(());
        }
        let pts_us = u64::try_from(pts_raw.max(0)).unwrap_or(0);
        let unit = match self.shared.order {
            DecodeOrder::LowLatency => self.pending.take(pts_us),
            DecodeOrder::Presentation => self.pending.remove(pts_us),
        };
        let info = PendingUnit::into_frame_info(unit, pts_us);
        let (image, buffer_size) =
            if format == self.av.pix_fmts.vaapi && self.av.pix_fmts.vaapi != AV_PIX_FMT_NONE {
                self.export_vaapi_dmabuf((width, height))?
            } else {
                let image = self.copy_software_frame(format, (width, height))?;
                (image, (width, height))
            };
        let crop = CropRect {
            x: 0,
            y: 0,
            width: width.min(buffer_size.0),
            height: height.min(buffer_size.1),
        };
        let range = (format == self.av.pix_fmts.yuvj420p
            && self.av.pix_fmts.yuvj420p != AV_PIX_FMT_NONE)
            .then_some(super::ColorRange::Full);
        let color = self.shared.resolve_color(None, range, None, crop.height);
        let frame = VideoFrame::new(info, buffer_size, crop, color, self.shared.rotation, image);
        self.shared.enqueue_frame(frame);
        self.shared.queue.stats.decoded(info.decode_time);
        Ok(())
    }

    fn export_vaapi_dmabuf(
        &self,
        visible: (u32, u32),
    ) -> Result<(Image, (u32, u32)), DecoderError> {
        let va = self.va.ok_or(DecoderError::Unavailable)?;
        // SAFETY: clones `self.frame` so the VA-API surface stays alive until `VideoFrame` drops,
        // syncs the surface on the worker thread, and exports its two DMABUF layers.
        let cloned = unsafe { (self.av.av_frame_clone)(self.frame) };
        if cloned.is_null() {
            return Err(DecoderError::Failed("av_frame_clone failed".into()));
        }
        let pin = unsafe { AvFramePin::new(cloned, self.av.av_frame_free) };
        let surface_id = unsafe {
            let head = &*cloned.cast::<AvFrameHead>();
            u32::try_from(head.data[3] as usize).unwrap_or(0) as VaSurfaceId
        };
        let sync = unsafe { (va.sync_surface)(self.va_display, surface_id) };
        if sync != VA_STATUS_SUCCESS {
            return Err(DecoderError::Failed(format!("vaSyncSurface: {sync}")));
        }
        let mut desc = VaDrmPrimeSurfaceDescriptor::default();
        let status = unsafe {
            (va.export_surface_handle)(
                self.va_display,
                surface_id,
                VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2,
                VA_EXPORT_SURFACE_READ_ONLY | VA_EXPORT_SURFACE_SEPARATE_LAYERS,
                ptr::from_mut(&mut desc).cast(),
            )
        };
        if status != VA_STATUS_SUCCESS || desc.num_layers < 2 || desc.num_objects == 0 {
            close_exported_fds(&desc);
            return Err(DecoderError::Failed(format!(
                "vaExportSurfaceHandle: status={status} layers={} objs={}",
                desc.num_layers, desc.num_objects
            )));
        }
        let format = if desc.fourcc == VA_FOURCC_P010 {
            LinuxPixelFormat::P010
        } else {
            LinuxPixelFormat::Nv12
        };
        let buf_w = desc.width.max(visible.0);
        let buf_h = desc.height.max(visible.1);
        let luma_layer = desc.layers[0];
        let chroma_layer = desc.layers[1];
        let luma_obj_idx = usize::try_from(luma_layer.object_index[0]).unwrap_or(0);
        let chroma_obj_idx = usize::try_from(chroma_layer.object_index[0]).unwrap_or(0);
        if luma_obj_idx >= desc.num_objects as usize || chroma_obj_idx >= desc.num_objects as usize
        {
            close_exported_fds(&desc);
            return Err(DecoderError::Failed("invalid VA-API object index".into()));
        }
        let luma_obj = desc.objects[luma_obj_idx];
        let chroma_obj = desc.objects[chroma_obj_idx];
        let mut fds = [-1; 4];
        let fd_count = (desc.num_objects as usize).min(4);
        for (dst, obj) in fds.iter_mut().zip(desc.objects.iter()).take(fd_count) {
            *dst = obj.fd;
        }
        let dmabuf = DmaBufFrame {
            luma: DmaBufPlane {
                fd: luma_obj.fd,
                offset: u64::from(luma_layer.offset[0]),
                stride: u64::from(luma_layer.pitch[0]),
                modifier: luma_obj.drm_format_modifier,
                width: buf_w,
                height: buf_h,
            },
            chroma: DmaBufPlane {
                fd: chroma_obj.fd,
                offset: u64::from(chroma_layer.offset[0]),
                stride: u64::from(chroma_layer.pitch[0]),
                modifier: chroma_obj.drm_format_modifier,
                width: buf_w.div_ceil(2),
                height: buf_h.div_ceil(2),
            },
            format,
        };
        Ok((
            Image::from_dmabuf(fds, fd_count, dmabuf, pin, &self.shared.live),
            (buf_w, buf_h),
        ))
    }

    fn copy_software_frame(
        &self,
        format: c_int,
        (width, height): (u32, u32),
    ) -> Result<Image, DecoderError> {
        let fmts = self.av.pix_fmts;
        // SAFETY: reads `data` and `linesize` of the currently received software `AVFrame`.
        let head = unsafe { &*self.frame.cast::<AvFrameHead>() };
        if format == fmts.nv12 || format == fmts.p010le {
            let pixel_format = if format == fmts.p010le {
                LinuxPixelFormat::P010
            } else {
                LinuxPixelFormat::Nv12
            };
            return Ok(copy_biplanar_software(
                head,
                (width, height),
                pixel_format,
                &self.software_pool,
                &self.shared.live,
            ));
        }
        if format == fmts.yuv420p || format == fmts.yuvj420p || format == fmts.yuv420p10le {
            let pixel_format = if format == fmts.yuv420p10le {
                LinuxPixelFormat::P010
            } else {
                LinuxPixelFormat::Nv12
            };
            return Ok(interleave_planar_420_software(
                head,
                (width, height),
                pixel_format,
                &self.software_pool,
                &self.shared.live,
            ));
        }
        Err(DecoderError::Failed(format!(
            "unsupported Linux software pixel format: {format}"
        )))
    }
}

fn find_decoder(av: &AvApi, codec: VideoCodec) -> Option<*const c_void> {
    for &name in av_decoder_names(codec) {
        // SAFETY: `name` is a static C string decoder name.
        let ptr = unsafe { (av.avcodec_find_decoder_by_name)(name.as_ptr()) };
        if !ptr.is_null() {
            return Some(ptr);
        }
    }
    None
}

fn try_attach_vaapi(
    av: &AvApi,
    ctx: *mut c_void,
    codec: VideoCodec,
    depth: usize,
) -> (Option<&'static VaApi>, *mut c_void, VaDisplay) {
    let Some(va) = va_api() else {
        return (None, ptr::null_mut(), ptr::null_mut());
    };
    if av.vaapi_device_type <= 0
        || av.pix_fmts.vaapi == AV_PIX_FMT_NONE
        || !probe_vaapi_codec(codec).is_some_and(|s| s.vld)
    {
        return (None, ptr::null_mut(), ptr::null_mut());
    }
    for node in render_nodes() {
        let mut hw_ctx: *mut c_void = ptr::null_mut();
        // SAFETY: creates an FFmpeg VA-API hardware device context on the DRM render node.
        let status = unsafe {
            (av.av_hwdevice_ctx_create)(
                &mut hw_ctx,
                av.vaapi_device_type,
                node.as_ptr(),
                ptr::null_mut(),
                0,
            )
        };
        if status < 0 || hw_ctx.is_null() {
            continue;
        }
        // SAFETY: extracts `VADisplay` from `AVHWDeviceContext -> AVVAAPIDeviceContext` and binds
        // a refcounted `hw_device_ctx` + `get_format` callback at the runtime-probed offsets.
        let va_display = unsafe {
            let buf = &*hw_ctx.cast::<AvBufferRef>();
            let dev = &*buf.data.cast::<AvHwDeviceContext>();
            *dev.hwctx.cast::<VaDisplay>()
        };
        if va_display.is_null() {
            unsafe { (av.av_buffer_unref)(&mut hw_ctx) };
            continue;
        }
        unsafe {
            let hw_ref = (av.av_buffer_ref)(hw_ctx);
            let hw_slot = ctx
                .cast::<u8>()
                .add(av.offsets.hw_device_ctx)
                .cast::<*mut c_void>();
            *hw_slot = hw_ref;
            let gf_slot = ctx
                .cast::<u8>()
                .add(av.offsets.get_format)
                .cast::<unsafe extern "C" fn(*mut c_void, *const c_int) -> c_int>();
            *gf_slot = select_vaapi_format;
            let extra = i64::try_from(depth + 4).unwrap_or(8);
            let _ = (av.av_opt_set_int)(ctx, c"extra_hw_frames".as_ptr(), extra, 0);
        }
        return (Some(va), hw_ctx, va_display);
    }
    (None, ptr::null_mut(), ptr::null_mut())
}

fn configure_codec_options(av: &AvApi, ctx: *mut c_void, order: DecodeOrder, hardware: bool) {
    // SAFETY: configures standard `AVCodecContext` options by name.
    unsafe {
        if order == DecodeOrder::LowLatency {
            let _ = (av.av_opt_set_int)(ctx, c"flags".as_ptr(), AV_CODEC_FLAG_LOW_DELAY, 0);
        }
        // Single-threaded mode on VA-API avoids extra surface-pool contention while keeping low delay.
        let threads = if hardware || order == DecodeOrder::LowLatency {
            1
        } else {
            0
        };
        let _ = (av.av_opt_set_int)(ctx, c"threads".as_ptr(), threads, 0);
    }
}

fn close_exported_fds(desc: &VaDrmPrimeSurfaceDescriptor) {
    for obj in desc.objects.iter().take((desc.num_objects as usize).min(4)) {
        if obj.fd >= 0 {
            // SAFETY: closes failed-export DMABUF file descriptors.
            unsafe { libc::close(obj.fd) };
        }
    }
}

struct SoftwareLayout {
    bpp: usize,
    row_y: usize,
    chroma_w: usize,
    chroma_h: usize,
    row_uv: usize,
}

fn prepare_software_slot(
    (width, height): (u32, u32),
    format: LinuxPixelFormat,
    pool: &SoftwarePool,
) -> (super::linux_frame::SoftwareSlot, SoftwareLayout) {
    let bpp: usize = match format {
        LinuxPixelFormat::Nv12 => 1,
        LinuxPixelFormat::P010 => 2,
    };
    let row_y = (width as usize) * bpp;
    let chroma_w = (width as usize).div_ceil(2);
    let chroma_h = (height as usize).div_ceil(2);
    let row_uv = chroma_w * 2 * bpp;
    let slot = take_software_slot(
        pool,
        row_y * (height as usize),
        row_uv * chroma_h,
        u32::try_from(row_y).unwrap_or(0),
        u32::try_from(row_uv).unwrap_or(0),
        format,
    );
    (
        slot,
        SoftwareLayout {
            bpp,
            row_y,
            chroma_w,
            chroma_h,
            row_uv,
        },
    )
}

fn copy_biplanar_software(
    head: &AvFrameHead,
    size: (u32, u32),
    format: LinuxPixelFormat,
    pool: &SoftwarePool,
    live: &LivePictures,
) -> Image {
    let (mut slot, layout) = prepare_software_slot(size, format, pool);
    let src_y_stride = usize::try_from(head.linesize[0]).unwrap_or(0);
    let src_uv_stride = usize::try_from(head.linesize[1]).unwrap_or(0);
    if !head.data[0].is_null() && src_y_stride >= layout.row_y {
        for y in 0..size.1 as usize {
            // SAFETY: `data[0]` holds `size.1` rows of `src_y_stride` bytes.
            unsafe {
                ptr::copy_nonoverlapping(
                    head.data[0].add(y * src_y_stride),
                    slot.luma.as_mut_ptr().add(y * layout.row_y),
                    layout.row_y,
                );
            }
        }
    }
    if !head.data[1].is_null() && src_uv_stride >= layout.row_uv {
        for y in 0..layout.chroma_h {
            // SAFETY: `data[1]` holds `chroma_h` rows of `src_uv_stride` bytes.
            unsafe {
                ptr::copy_nonoverlapping(
                    head.data[1].add(y * src_uv_stride),
                    slot.chroma.as_mut_ptr().add(y * layout.row_uv),
                    layout.row_uv,
                );
            }
        }
    }
    Image::from_software(slot, pool, live)
}

fn interleave_planar_420_software(
    head: &AvFrameHead,
    size: (u32, u32),
    format: LinuxPixelFormat,
    pool: &SoftwarePool,
    live: &LivePictures,
) -> Image {
    let (mut slot, layout) = prepare_software_slot(size, format, pool);
    let (bpp, row_y, chroma_w, chroma_h, row_uv) = (
        layout.bpp,
        layout.row_y,
        layout.chroma_w,
        layout.chroma_h,
        layout.row_uv,
    );
    let src_y_stride = usize::try_from(head.linesize[0]).unwrap_or(0);
    let src_u_stride = usize::try_from(head.linesize[1]).unwrap_or(0);
    let src_v_stride = usize::try_from(head.linesize[2]).unwrap_or(0);
    if !head.data[0].is_null() && src_y_stride >= row_y {
        for y in 0..size.1 as usize {
            // SAFETY: `data[0]` holds `height` rows of `src_y_stride` bytes.
            let src =
                unsafe { std::slice::from_raw_parts(head.data[0].add(y * src_y_stride), row_y) };
            let dst = &mut slot.luma[y * row_y..(y + 1) * row_y];
            if format == LinuxPixelFormat::P010 {
                let (src_pxs, _) = src.as_chunks::<2>();
                let (dst_pxs, _) = dst.as_chunks_mut::<2>();
                for (&src_px, dst_px) in src_pxs.iter().zip(dst_pxs.iter_mut()) {
                    let sample = u16::from_le_bytes(src_px) << 6;
                    *dst_px = sample.to_le_bytes();
                }
            } else {
                dst.copy_from_slice(src);
            }
        }
    }
    if !head.data[1].is_null()
        && !head.data[2].is_null()
        && src_u_stride >= chroma_w * bpp
        && src_v_stride >= chroma_w * bpp
    {
        for y in 0..chroma_h {
            // SAFETY: `data[1]` and `data[2]` hold `chroma_h` rows of U and V samples.
            let u_row = unsafe {
                std::slice::from_raw_parts(head.data[1].add(y * src_u_stride), chroma_w * bpp)
            };
            let v_row = unsafe {
                std::slice::from_raw_parts(head.data[2].add(y * src_v_stride), chroma_w * bpp)
            };
            let dst = &mut slot.chroma[y * row_uv..(y + 1) * row_uv];
            if format == LinuxPixelFormat::P010 {
                let (u_pxs, _) = u_row.as_chunks::<2>();
                let (v_pxs, _) = v_row.as_chunks::<2>();
                let (out_pxs, _) = dst.as_chunks_mut::<4>();
                for ((&u, &v), out) in u_pxs.iter().zip(v_pxs.iter()).zip(out_pxs.iter_mut()) {
                    let su = (u16::from_le_bytes(u) << 6).to_le_bytes();
                    let sv = (u16::from_le_bytes(v) << 6).to_le_bytes();
                    *out = [su[0], su[1], sv[0], sv[1]];
                }
            } else {
                let (out_pxs, _) = dst.as_chunks_mut::<2>();
                for ((&u, &v), out) in u_row.iter().zip(v_row.iter()).zip(out_pxs.iter_mut()) {
                    *out = [u, v];
                }
            }
        }
    }
    Image::from_software(slot, pool, live)
}
