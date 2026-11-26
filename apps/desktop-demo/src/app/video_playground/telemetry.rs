// Independent live performance telemetry measuring:
// 1. Decoder FPS & decoded pixels/sec
// 2. Compositor FPS & frame time ms
// 3. Dropped frames (decoder + presentation)
// 4. GPU video texture memory & process RSS
// 5. Process CPU utilization %
// 6. Frame-copy overhead (zero-copy GPU import latency vs CPU upload bytes/sec)

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, PoisonError,
    },
    time::Duration,
};

use cranpose::prelude::{ColorPrimaries, ColorRange, ColorTransfer, Player, VideoColorInfo};
use portable_atomic::AtomicU64;
use web_time::Instant;

const HISTORY_LEN: usize = 48;
const ROLLING_WINDOW_TICKS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameTransferMode {
    ZeroCopyGpu,
    CpuUpload,
}

impl FrameTransferMode {
    pub(super) const fn from_zero_copy(zero_copy: bool) -> Self {
        if zero_copy {
            Self::ZeroCopyGpu
        } else {
            Self::CpuUpload
        }
    }

    pub(super) const fn is_zero_copy(self) -> bool {
        matches!(self, Self::ZeroCopyGpu)
    }
}

struct WindowTick {
    dt: f32,
    decoded_frames: u64,
    decoded_pixels: u64,
    compositor_frames: u64,
    import_ns: u64,
    import_samples: u64,
    cpu_copy_bytes: u64,
    cpu_dt: Option<f32>,
}

pub(super) struct TelemetryCounters {
    presented_frames: AtomicU64,
    presented_pixels: AtomicU64,
    present_dropped: AtomicU64,
    import_ns_total: AtomicU64,
    import_samples: AtomicU64,
    import_ns_max: AtomicU64,
    cpu_copy_bytes: AtomicU64,
    compositor_frames: AtomicU64,
    last_width: AtomicU32,
    last_height: AtomicU32,
    last_color: Mutex<Option<VideoColorInfo>>,
}

impl PartialEq for TelemetryCounters {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl TelemetryCounters {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            presented_frames: AtomicU64::new(0),
            presented_pixels: AtomicU64::new(0),
            present_dropped: AtomicU64::new(0),
            import_ns_total: AtomicU64::new(0),
            import_samples: AtomicU64::new(0),
            import_ns_max: AtomicU64::new(0),
            cpu_copy_bytes: AtomicU64::new(0),
            compositor_frames: AtomicU64::new(0),
            last_width: AtomicU32::new(0),
            last_height: AtomicU32::new(0),
            last_color: Mutex::new(None),
        })
    }

    pub(super) fn reset(&self) {
        self.presented_frames.store(0, Ordering::Relaxed);
        self.presented_pixels.store(0, Ordering::Relaxed);
        self.present_dropped.store(0, Ordering::Relaxed);
        self.import_ns_total.store(0, Ordering::Relaxed);
        self.import_samples.store(0, Ordering::Relaxed);
        self.import_ns_max.store(0, Ordering::Relaxed);
        self.cpu_copy_bytes.store(0, Ordering::Relaxed);
        self.compositor_frames.store(0, Ordering::Relaxed);
        self.last_width.store(0, Ordering::Relaxed);
        self.last_height.store(0, Ordering::Relaxed);
        *self
            .last_color
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }

    pub(super) fn record_compositor_frame(&self) {
        self.compositor_frames.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_presented_meta(
        &self,
        size: (u32, u32),
        color: VideoColorInfo,
        import_elapsed: Duration,
        transfer: FrameTransferMode,
    ) {
        let (w, h) = size;
        let pixels = u64::from(w).saturating_mul(u64::from(h));
        self.presented_frames.fetch_add(1, Ordering::Relaxed);
        self.presented_pixels.fetch_add(pixels, Ordering::Relaxed);
        self.last_width.store(w, Ordering::Relaxed);
        self.last_height.store(h, Ordering::Relaxed);
        let ns = u64::try_from(import_elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.import_ns_total.fetch_add(ns, Ordering::Relaxed);
        self.import_samples.fetch_add(1, Ordering::Relaxed);
        self.import_ns_max.fetch_max(ns, Ordering::Relaxed);
        if !transfer.is_zero_copy() {
            // NV12 / 4:2:0 software fallback uploads 1.5 bytes per pixel across CPU->GPU bus.
            self.cpu_copy_bytes
                .fetch_add(pixels.saturating_mul(3) / 2, Ordering::Relaxed);
        }
        *self
            .last_color
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(color);
    }

    pub(super) fn record_present_drop(&self) {
        self.present_dropped.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TelemetrySnapshot {
    pub(super) decoder_fps: f32,
    pub(super) decoded_mpix_per_sec: f32,
    pub(super) compositor_fps: f32,
    pub(super) compositor_frame_ms: f32,
    pub(super) decoder_dropped: u64,
    pub(super) present_dropped: u64,
    pub(super) gpu_video_mb: f32,
    pub(super) process_rss_mb: Option<f32>,
    pub(super) cpu_percent: Option<f32>,
    pub(super) mean_import_us: f32,
    pub(super) max_import_us: f32,
    pub(super) mean_decode_ms: f32,
    pub(super) cpu_copy_mb_per_sec: f32,
    pub(super) zero_copy_gpu: bool,
    pub(super) active_decoders: usize,
    pub(super) active_surfaces: usize,
    pub(super) sample_size: (u32, u32),
    pub(super) color_summary: String,
    pub(super) decoder_history: Vec<f32>,
    pub(super) compositor_history: Vec<f32>,
}

impl Default for TelemetrySnapshot {
    fn default() -> Self {
        Self {
            decoder_fps: 0.0,
            decoded_mpix_per_sec: 0.0,
            compositor_fps: 0.0,
            compositor_frame_ms: 0.0,
            decoder_dropped: 0,
            present_dropped: 0,
            gpu_video_mb: 0.0,
            process_rss_mb: None,
            cpu_percent: None,
            mean_import_us: 0.0,
            max_import_us: 0.0,
            mean_decode_ms: 0.0,
            cpu_copy_mb_per_sec: 0.0,
            zero_copy_gpu: true,
            active_decoders: 0,
            active_surfaces: 0,
            sample_size: (0, 0),
            color_summary: "Probing stream...".to_owned(),
            decoder_history: Vec::new(),
            compositor_history: Vec::new(),
        }
    }
}

pub(super) struct TelemetrySampler {
    last_sample_at: Instant,
    last_decoded_frames: u64,
    last_presented_frames: u64,
    last_presented_pixels: u64,
    last_compositor_frames: u64,
    last_import_ns: u64,
    last_import_samples: u64,
    last_cpu_copy_bytes: u64,
    last_cpu_time: Option<Duration>,
    window: VecDeque<WindowTick>,
    decoder_history: VecDeque<f32>,
    compositor_history: VecDeque<f32>,
}

impl TelemetrySampler {
    pub(super) fn new() -> Self {
        Self {
            last_sample_at: Instant::now(),
            last_decoded_frames: 0,
            last_presented_frames: 0,
            last_presented_pixels: 0,
            last_compositor_frames: 0,
            last_import_ns: 0,
            last_import_samples: 0,
            last_cpu_copy_bytes: 0,
            last_cpu_time: cranpose_services::device_info().process_cpu_time(),
            window: VecDeque::with_capacity(ROLLING_WINDOW_TICKS),
            decoder_history: VecDeque::with_capacity(HISTORY_LEN),
            compositor_history: VecDeque::with_capacity(HISTORY_LEN),
        }
    }

    pub(super) fn sample(
        &mut self,
        counters: &TelemetryCounters,
        players: &[Player],
        active_surfaces: usize,
        transfer: FrameTransferMode,
    ) -> TelemetrySnapshot {
        let (decoder_dropped, mean_decode_ms, live_players, tick) =
            self.collect_tick(counters, players);
        if self.window.len() >= ROLLING_WINDOW_TICKS {
            self.window.pop_front();
        }
        self.window.push_back(tick);

        let rates = self.summarize_window(counters);
        push_history(&mut self.decoder_history, rates.decoder_fps);
        push_history(&mut self.compositor_history, rates.compositor_fps);

        let process_rss_mb = cranpose_services::device_info()
            .resident_memory_bytes()
            .map(|b| b as f32 / (1024.0 * 1024.0));
        let color_summary = counters
            .last_color
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map_or_else(|| "Awaiting frame...".to_owned(), format_color_info);

        TelemetrySnapshot {
            decoder_fps: rates.decoder_fps,
            decoded_mpix_per_sec: rates.decoded_mpix_per_sec,
            compositor_fps: rates.compositor_fps,
            compositor_frame_ms: rates.compositor_frame_ms,
            decoder_dropped,
            present_dropped: counters.present_dropped.load(Ordering::Relaxed),
            gpu_video_mb: live_gpu_video_bytes() as f32 / (1024.0 * 1024.0),
            process_rss_mb,
            cpu_percent: rates.cpu_percent,
            mean_import_us: rates.mean_import_us,
            max_import_us: rates.max_import_us,
            mean_decode_ms,
            cpu_copy_mb_per_sec: rates.cpu_copy_mb_per_sec,
            zero_copy_gpu: transfer.is_zero_copy(),
            active_decoders: live_players,
            active_surfaces,
            sample_size: (
                counters.last_width.load(Ordering::Relaxed),
                counters.last_height.load(Ordering::Relaxed),
            ),
            color_summary,
            decoder_history: self.decoder_history.iter().copied().collect(),
            compositor_history: self.compositor_history.iter().copied().collect(),
        }
    }

    fn collect_tick(
        &mut self,
        counters: &TelemetryCounters,
        players: &[Player],
    ) -> (u64, f32, usize, WindowTick) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_sample_at)
            .as_secs_f32()
            .max(0.05);
        self.last_sample_at = now;

        let (decoded_total, decoder_dropped, mean_decode_ms, live_players) =
            aggregate_player_stats(players);
        let decoded_delta = decoded_total.saturating_sub(self.last_decoded_frames);
        self.last_decoded_frames = decoded_total;

        let presented_total = counters.presented_frames.load(Ordering::Relaxed);
        let presented_delta = presented_total.saturating_sub(self.last_presented_frames);
        self.last_presented_frames = presented_total;

        let pixels_total = counters.presented_pixels.load(Ordering::Relaxed);
        let pixels_delta = pixels_total.saturating_sub(self.last_presented_pixels);
        self.last_presented_pixels = pixels_total;

        // Use real decoder counter delta when available, falling back to presented count on
        // backends that push frames directly without an inner decoder queue.
        let effective_decoded = decoded_delta.max(presented_delta);
        let sample_w = counters.last_width.load(Ordering::Relaxed);
        let sample_h = counters.last_height.load(Ordering::Relaxed);
        let decoded_pixels = if pixels_delta > 0 {
            pixels_delta
        } else {
            effective_decoded
                .saturating_mul(u64::from(sample_w))
                .saturating_mul(u64::from(sample_h))
        };

        let comp_total = counters.compositor_frames.load(Ordering::Relaxed);
        let comp_delta = comp_total.saturating_sub(self.last_compositor_frames);
        self.last_compositor_frames = comp_total;

        let import_ns = counters.import_ns_total.load(Ordering::Relaxed);
        let import_samples = counters.import_samples.load(Ordering::Relaxed);
        let import_ns_delta = import_ns.saturating_sub(self.last_import_ns);
        let import_samples_delta = import_samples.saturating_sub(self.last_import_samples);
        self.last_import_ns = import_ns;
        self.last_import_samples = import_samples;

        let copy_total = counters.cpu_copy_bytes.load(Ordering::Relaxed);
        let copy_delta = copy_total.saturating_sub(self.last_cpu_copy_bytes);
        self.last_cpu_copy_bytes = copy_total;

        let cpu_now = cranpose_services::device_info().process_cpu_time();
        let cpu_dt = self
            .last_cpu_time
            .zip(cpu_now)
            .map(|(prev, curr)| curr.saturating_sub(prev).as_secs_f32());
        self.last_cpu_time = cpu_now;

        let tick = WindowTick {
            dt,
            decoded_frames: effective_decoded,
            decoded_pixels,
            compositor_frames: comp_delta,
            import_ns: import_ns_delta,
            import_samples: import_samples_delta,
            cpu_copy_bytes: copy_delta,
            cpu_dt,
        };
        (decoder_dropped, mean_decode_ms, live_players, tick)
    }

    fn summarize_window(&self, counters: &TelemetryCounters) -> WindowRates {
        let win_dt: f32 = self.window.iter().map(|t| t.dt).sum::<f32>().max(0.05);
        let win_dec_frames: u64 = self.window.iter().map(|t| t.decoded_frames).sum();
        let win_dec_pixels: u64 = self.window.iter().map(|t| t.decoded_pixels).sum();
        let win_comp_frames: u64 = self.window.iter().map(|t| t.compositor_frames).sum();
        let win_import_ns: u64 = self.window.iter().map(|t| t.import_ns).sum();
        let win_import_samples: u64 = self.window.iter().map(|t| t.import_samples).sum();
        let win_copy_bytes: u64 = self.window.iter().map(|t| t.cpu_copy_bytes).sum();
        let (win_cpu_sum, win_cpu_dt) = self.window.iter().fold((0.0_f32, 0.0_f32), |acc, t| {
            t.cpu_dt.map_or(acc, |c| (acc.0 + c, acc.1 + t.dt))
        });

        let compositor_fps = win_comp_frames as f32 / win_dt;
        WindowRates {
            decoder_fps: win_dec_frames as f32 / win_dt,
            decoded_mpix_per_sec: win_dec_pixels as f32 / win_dt / 1_000_000.0,
            compositor_fps,
            compositor_frame_ms: if compositor_fps > 0.1 {
                1000.0 / compositor_fps
            } else {
                0.0
            },
            mean_import_us: if win_import_samples > 0 {
                win_import_ns as f32 / win_import_samples as f32 / 1000.0
            } else {
                0.0
            },
            max_import_us: counters.import_ns_max.load(Ordering::Relaxed) as f32 / 1000.0,
            cpu_copy_mb_per_sec: win_copy_bytes as f32 / win_dt / (1024.0 * 1024.0),
            cpu_percent: (win_cpu_dt > 0.0).then_some((win_cpu_sum / win_cpu_dt) * 100.0),
        }
    }
}

struct WindowRates {
    decoder_fps: f32,
    decoded_mpix_per_sec: f32,
    compositor_fps: f32,
    compositor_frame_ms: f32,
    mean_import_us: f32,
    max_import_us: f32,
    cpu_copy_mb_per_sec: f32,
    cpu_percent: Option<f32>,
}

fn aggregate_player_stats(players: &[Player]) -> (u64, u64, f32, usize) {
    let mut decoded = 0_u64;
    let mut dropped = 0_u64;
    let mut decode_ms_sum = 0.0_f32;
    let mut decode_count = 0_u32;
    let mut live = 0_usize;
    for player in players {
        if let Some(frames) = player.decoded_frames() {
            live += 1;
            let stats = frames.stats();
            decoded = decoded.saturating_add(stats.decoded);
            dropped = dropped
                .saturating_add(stats.dropped)
                .saturating_add(stats.late)
                .saturating_add(stats.refused);
            if stats.mean_decode > Duration::ZERO {
                decode_ms_sum += stats.mean_decode.as_secs_f32() * 1000.0;
                decode_count += 1;
            }
        }
    }
    let mean_decode_ms = if decode_count > 0 {
        decode_ms_sum / decode_count as f32
    } else {
        0.0
    };
    (decoded, dropped, mean_decode_ms, live)
}

fn push_history(buf: &mut VecDeque<f32>, value: f32) {
    if buf.len() >= HISTORY_LEN {
        buf.pop_front();
    }
    buf.push_back(value);
}

fn live_gpu_video_bytes() -> u64 {
    #[cfg(feature = "renderer-wgpu")]
    {
        cranpose::external_frame_gpu_bytes()
    }
    #[cfg(not(feature = "renderer-wgpu"))]
    {
        0
    }
}

fn format_color_info(info: VideoColorInfo) -> String {
    let primaries = match info.primaries {
        ColorPrimaries::Bt601 => "BT.601",
        ColorPrimaries::Bt709 => "BT.709",
        ColorPrimaries::Bt2020 => "BT.2020",
    };
    let transfer = match info.transfer {
        ColorTransfer::Sdr => "SDR",
        ColorTransfer::Pq => "HDR10 PQ",
        ColorTransfer::Hlg => "HLG HDR",
    };
    let range = match info.range {
        ColorRange::Limited => "Limited",
        ColorRange::Full => "Full",
    };
    format!("{primaries} · {transfer} · {range}")
}
