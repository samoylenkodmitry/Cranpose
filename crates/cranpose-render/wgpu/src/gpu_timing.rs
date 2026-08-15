use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;

const MAX_QUERIES: u32 = 512;
const READBACK_SLOTS: usize = 3;
const QUERY_BYTES: u64 = 8;
const MAX_REPORTED_BUCKETS: usize = 24;
const MAX_LISTED_PASSES: usize = 40;

pub fn timing_window_frames() -> Option<u32> {
    static WINDOW: OnceLock<Option<u32>> = OnceLock::new();
    *WINDOW.get_or_init(|| {
        let raw = std::env::var("CRANPOSE_GPU_PASS_TIMING").ok()?;
        let raw = raw.trim().to_owned();
        if raw.is_empty() || raw == "0" || raw == "off" || raw == "false" {
            return None;
        }
        Some(raw.parse::<u32>().unwrap_or(120).clamp(1, 10_000))
    })
}

pub fn requested_features(adapter_features: wgpu::Features) -> wgpu::Features {
    let Some(window) = timing_window_frames() else {
        return wgpu::Features::empty();
    };
    log::warn!(
        "[gpu-pass] window={window} adapter timestamp_query={} inside_encoders={} inside_passes={}",
        adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY),
        adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
        adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
    );
    if !adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY) {
        return wgpu::Features::empty();
    }
    let mut asked = wgpu::Features::TIMESTAMP_QUERY;
    if adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
        asked |= wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    }
    asked
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum GpuSpanKind {
    Frame,
    BlurHorizontal,
    BlurVertical,
    BlurRoundedMask,
    Composite,
    Content,
}

impl GpuSpanKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            GpuSpanKind::Frame => "frame",
            GpuSpanKind::BlurHorizontal => "blur_h",
            GpuSpanKind::BlurVertical => "blur_v",
            GpuSpanKind::BlurRoundedMask => "blur_mask",
            GpuSpanKind::Composite => "composite",
            GpuSpanKind::Content => "content",
        }
    }

    pub(crate) fn is_blur(self) -> bool {
        matches!(
            self,
            GpuSpanKind::BlurHorizontal | GpuSpanKind::BlurVertical | GpuSpanKind::BlurRoundedMask
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuSpanId(usize);

pub(crate) struct PassTimestamps {
    query_set: wgpu::QuerySet,
    begin: u32,
    end: u32,
}

impl PassTimestamps {
    pub(crate) fn writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(self.begin),
            end_of_pass_write_index: Some(self.end),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpanRecord {
    kind: GpuSpanKind,
    width: u32,
    height: u32,
    begin: u32,
    end: Option<u32>,
}

impl SpanRecord {
    #[cfg(test)]
    pub(crate) fn for_test(
        kind: GpuSpanKind,
        width: u32,
        height: u32,
        begin: u32,
        end: Option<u32>,
    ) -> Self {
        Self {
            kind,
            width,
            height,
            begin,
            end,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpanTime {
    pub(crate) kind: GpuSpanKind,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) ms: f64,
}

pub(crate) fn decode_spans(spans: &[SpanRecord], ticks: &[u64], period_ns: f32) -> Vec<SpanTime> {
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        let Some(end) = span.end else {
            continue;
        };
        let (Some(begin_tick), Some(end_tick)) = (
            ticks.get(span.begin as usize).copied(),
            ticks.get(end as usize).copied(),
        ) else {
            continue;
        };
        if end_tick < begin_tick {
            continue;
        }
        let ms = (end_tick - begin_tick) as f64 * f64::from(period_ns) / 1_000_000.0;
        out.push(SpanTime {
            kind: span.kind,
            width: span.width,
            height: span.height,
            ms,
        });
    }
    out
}

#[derive(Default)]
pub(crate) struct GpuFrameTimer {
    state: Option<TimerState>,
    sizes: Option<SizeReport>,
    refused: bool,
}

#[derive(Default)]
pub(crate) struct SizeReport {
    window: u32,
    frames: u32,
    buckets: BTreeMap<(GpuSpanKind, u32, u32), u32>,
    frame: Vec<(GpuSpanKind, u32, u32)>,
    last_frame: Vec<(GpuSpanKind, u32, u32)>,
}

impl SizeReport {
    pub(crate) fn new(window: u32) -> Self {
        Self {
            window,
            ..Self::default()
        }
    }

    pub(crate) fn record(&mut self, kind: GpuSpanKind, width: u32, height: u32) {
        if self.frame.len() < MAX_LISTED_PASSES {
            self.frame.push((kind, width, height));
        }
    }

    pub(crate) fn end_frame(&mut self) {
        self.frames += 1;
        for entry in &self.frame {
            *self.buckets.entry(*entry).or_default() += 1;
        }
        std::mem::swap(&mut self.last_frame, &mut self.frame);
        self.frame.clear();
        if self.frames >= self.window {
            self.print();
            let window = self.window;
            *self = Self::new(window);
        }
    }

    fn print(&self) {
        if self.frames == 0 {
            return;
        }
        let frames = f64::from(self.frames);
        log::warn!(
            "[gpu-pass] sizes only, frames={}, the adapter carries no timestamp query",
            self.frames
        );
        for ((kind, width, height), count) in &self.buckets {
            log::warn!(
                "[gpu-pass]   {} {}x{} per_frame_n={:.2}",
                kind.label(),
                width,
                height,
                f64::from(*count) / frames,
            );
        }
        let mut line = String::new();
        for (kind, width, height) in &self.last_frame {
            if !line.is_empty() {
                line.push_str("; ");
            }
            line.push_str(&format!("{} {width}x{height}", kind.label()));
        }
        if !line.is_empty() {
            log::warn!("[gpu-pass]   one frame: {line}");
        }
    }
}

struct TimerState {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    filled_slot: Option<usize>,
    next_query: u32,
    spans: Vec<SpanRecord>,
    frame_span: Option<GpuSpanId>,
    dropped_spans: u32,
    period_ns: f32,
    window: u32,
    inside_encoders: bool,
    report: Report,
}

struct Slot {
    buffer: wgpu::Buffer,
    ready: Arc<AtomicBool>,
    pending: Option<Pending>,
}

struct Pending {
    spans: Vec<SpanRecord>,
    queries: u32,
    mapped: bool,
}

impl GpuFrameTimer {
    pub(crate) fn begin_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if self.refused {
            return;
        }
        let Some(window) = timing_window_frames() else {
            self.refused = true;
            return;
        };
        if self.state.is_none() {
            if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
                if self.sizes.is_none() {
                    log::warn!(
                        "[gpu-pass] the device has no timestamp query, the report carries pass sizes alone"
                    );
                    self.sizes = Some(SizeReport::new(window));
                }
                return;
            }
            let inside_encoders = device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
            self.state = Some(TimerState::new(device, queue, window, inside_encoders));
            log::warn!(
                "[gpu-pass] per pass timing on, window={window} frames, frame span={inside_encoders}"
            );
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.next_query = 0;
        state.spans.clear();
        state.dropped_spans = 0;
        state.filled_slot = None;
        state.frame_span = None;
        if state.inside_encoders {
            let frame_span = state.open_encoder_span(encoder, GpuSpanKind::Frame, 0, 0);
            state.frame_span = frame_span;
        }
    }

    pub(crate) fn pass_timestamps(
        &mut self,
        kind: GpuSpanKind,
        width: u32,
        height: u32,
    ) -> Option<PassTimestamps> {
        if let Some(sizes) = self.sizes.as_mut() {
            sizes.record(kind, width, height);
            return None;
        }
        self.state.as_mut()?.open_pass(kind, width, height)
    }

    pub(crate) fn end_frame(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(sizes) = self.sizes.as_mut() {
            sizes.end_frame();
            return;
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if let Some(frame_span) = state.frame_span.take() {
            state.close(encoder, frame_span);
        }
        state.resolve_into_slot(encoder);
    }

    pub(crate) fn after_submit(&mut self, device: &wgpu::Device) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.map_filled_slot();
        let _ = device.poll(wgpu::PollType::Poll);
        state.drain_ready_slots();
    }
}

impl TimerState {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window: u32,
        inside_encoders: bool,
    ) -> Self {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("Gpu Pass Timing Query Set"),
            ty: wgpu::QueryType::Timestamp,
            count: MAX_QUERIES,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gpu Pass Timing Resolve Buffer"),
            size: u64::from(MAX_QUERIES) * QUERY_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..READBACK_SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Gpu Pass Timing Readback Buffer"),
                    size: u64::from(MAX_QUERIES) * QUERY_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                ready: Arc::new(AtomicBool::new(false)),
                pending: None,
            })
            .collect::<Vec<_>>();
        Self {
            query_set,
            resolve,
            slots,
            filled_slot: None,
            next_query: 0,
            spans: Vec::with_capacity(64),
            frame_span: None,
            dropped_spans: 0,
            period_ns: queue.get_timestamp_period(),
            window,
            inside_encoders,
            report: Report::default(),
        }
    }

    fn open_pass(&mut self, kind: GpuSpanKind, width: u32, height: u32) -> Option<PassTimestamps> {
        if self.next_query + 2 > MAX_QUERIES {
            self.dropped_spans += 1;
            return None;
        }
        let begin = self.next_query;
        let end = begin + 1;
        self.next_query += 2;
        self.spans.push(SpanRecord {
            kind,
            width,
            height,
            begin,
            end: Some(end),
        });
        Some(PassTimestamps {
            query_set: self.query_set.clone(),
            begin,
            end,
        })
    }

    fn open_encoder_span(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        kind: GpuSpanKind,
        width: u32,
        height: u32,
    ) -> Option<GpuSpanId> {
        if self.next_query + 2 > MAX_QUERIES {
            self.dropped_spans += 1;
            return None;
        }
        let begin = self.next_query;
        self.next_query += 1;
        encoder.write_timestamp(&self.query_set, begin);
        let id = GpuSpanId(self.spans.len());
        self.spans.push(SpanRecord {
            kind,
            width,
            height,
            begin,
            end: None,
        });
        Some(id)
    }

    fn close(&mut self, encoder: &mut wgpu::CommandEncoder, span: GpuSpanId) {
        if self.next_query >= MAX_QUERIES {
            return;
        }
        let end = self.next_query;
        self.next_query += 1;
        encoder.write_timestamp(&self.query_set, end);
        if let Some(record) = self.spans.get_mut(span.0) {
            record.end = Some(end);
        }
    }

    fn resolve_into_slot(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let queries = self.next_query;
        if queries == 0 {
            return;
        }
        let Some(slot_index) = self.slots.iter().position(|slot| slot.pending.is_none()) else {
            return;
        };
        let bytes = u64::from(queries) * QUERY_BYTES;
        encoder.resolve_query_set(&self.query_set, 0..queries, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.slots[slot_index].buffer, 0, bytes);
        self.slots[slot_index].ready.store(false, Ordering::Release);
        self.slots[slot_index].pending = Some(Pending {
            spans: self.spans.clone(),
            queries,
            mapped: false,
        });
        self.filled_slot = Some(slot_index);
    }

    fn map_filled_slot(&mut self) {
        let Some(index) = self.filled_slot.take() else {
            return;
        };
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        let Some(pending) = slot.pending.as_mut() else {
            return;
        };
        if pending.mapped {
            return;
        }
        pending.mapped = true;
        let bytes = u64::from(pending.queries) * QUERY_BYTES;
        let ready = Arc::clone(&slot.ready);
        slot.buffer
            .slice(0..bytes)
            .map_async(wgpu::MapMode::Read, move |result| {
                if result.is_ok() {
                    ready.store(true, Ordering::Release);
                }
            });
    }

    fn drain_ready_slots(&mut self) {
        for index in 0..self.slots.len() {
            if !self.slots[index].ready.load(Ordering::Acquire) {
                continue;
            }
            let Some(pending) = self.slots[index].pending.take() else {
                self.slots[index].ready.store(false, Ordering::Release);
                continue;
            };
            let bytes = u64::from(pending.queries) * QUERY_BYTES;
            let ticks = {
                let view = self.slots[index].buffer.slice(0..bytes).get_mapped_range();
                view.chunks_exact(8)
                    .map(|chunk| {
                        u64::from_le_bytes([
                            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6],
                            chunk[7],
                        ])
                    })
                    .collect::<Vec<_>>()
            };
            self.slots[index].buffer.unmap();
            self.slots[index].ready.store(false, Ordering::Release);
            let times = decode_spans(&pending.spans, &ticks, self.period_ns);
            self.report.add_frame(&times);
            if self.report.frames >= self.window {
                self.report.print(self.dropped_spans);
                self.report = Report::default();
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Report {
    frames: u32,
    frame_ms: f64,
    buckets: BTreeMap<(GpuSpanKind, u32, u32), Bucket>,
    last_frame: Vec<SpanTime>,
}

#[derive(Default, Clone, Copy)]
struct Bucket {
    count: u32,
    ms: f64,
}

impl Report {
    pub(crate) fn add_frame(&mut self, times: &[SpanTime]) {
        self.frames += 1;
        self.last_frame.clear();
        for time in times {
            if time.kind == GpuSpanKind::Frame {
                self.frame_ms += time.ms;
                continue;
            }
            let bucket = self
                .buckets
                .entry((time.kind, time.width, time.height))
                .or_default();
            bucket.count += 1;
            bucket.ms += time.ms;
            if self.last_frame.len() < MAX_LISTED_PASSES {
                self.last_frame.push(*time);
            }
        }
    }

    fn spans_ms(&self) -> f64 {
        self.buckets.values().map(|bucket| bucket.ms).sum()
    }

    fn blur_ms(&self) -> f64 {
        self.buckets
            .iter()
            .filter(|((kind, _, _), _)| kind.is_blur())
            .map(|(_, bucket)| bucket.ms)
            .sum()
    }

    fn composite_ms(&self) -> f64 {
        self.buckets
            .iter()
            .filter(|((kind, _, _), _)| *kind == GpuSpanKind::Composite)
            .map(|(_, bucket)| bucket.ms)
            .sum()
    }

    pub(crate) fn print(&self, dropped_spans: u32) {
        if self.frames == 0 {
            return;
        }
        let frames = f64::from(self.frames);
        let frame_ms = self.frame_ms / frames;
        let spans_ms = self.spans_ms() / frames;
        log::warn!(
            "[gpu-pass] frames={} frame_ms={frame_ms:.3} spans_ms={spans_ms:.3} blur_ms={:.3} composite_ms={:.3} rest_ms={:.3} dropped={dropped_spans}",
            self.frames,
            self.blur_ms() / frames,
            self.composite_ms() / frames,
            (self.frame_ms - self.spans_ms()).max(0.0) / frames,
        );
        for (index, ((kind, width, height), bucket)) in self.buckets.iter().enumerate() {
            if index >= MAX_REPORTED_BUCKETS {
                log::warn!("[gpu-pass]   and {} more sizes", self.buckets.len() - index);
                break;
            }
            log::warn!(
                "[gpu-pass]   {} {}x{} per_frame_n={:.2} per_frame_ms={:.3} per_pass_ms={:.3}",
                kind.label(),
                width,
                height,
                f64::from(bucket.count) / frames,
                bucket.ms / frames,
                bucket.ms / f64::from(bucket.count.max(1)),
            );
        }
        let mut line = String::new();
        for time in &self.last_frame {
            if !line.is_empty() {
                line.push_str("; ");
            }
            line.push_str(&format!(
                "{} {}x{} {:.3}",
                time.kind.label(),
                time.width,
                time.height,
                time.ms
            ));
        }
        if !line.is_empty() {
            log::warn!("[gpu-pass]   one frame: {line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_pair_becomes_milliseconds() {
        let spans = vec![SpanRecord::for_test(
            GpuSpanKind::BlurHorizontal,
            270,
            100,
            0,
            Some(1),
        )];
        let times = decode_spans(&spans, &[1_000, 2_000_000], 1.0);
        assert_eq!(times.len(), 1);
        assert_eq!(times[0].kind, GpuSpanKind::BlurHorizontal);
        assert_eq!(times[0].width, 270);
        assert!((times[0].ms - 1.999).abs() < 0.001, "{}", times[0].ms);
    }

    #[test]
    fn a_span_without_an_end_is_left_out() {
        let spans = vec![SpanRecord::for_test(GpuSpanKind::Composite, 8, 8, 0, None)];
        assert!(decode_spans(&spans, &[10, 20], 1.0).is_empty());
    }

    #[test]
    fn a_backward_pair_is_left_out() {
        let spans = vec![SpanRecord::for_test(
            GpuSpanKind::Composite,
            8,
            8,
            0,
            Some(1),
        )];
        assert!(decode_spans(&spans, &[20, 10], 1.0).is_empty());
    }

    #[test]
    fn a_missing_tick_is_left_out() {
        let spans = vec![SpanRecord::for_test(
            GpuSpanKind::Composite,
            8,
            8,
            0,
            Some(5),
        )];
        assert!(decode_spans(&spans, &[20, 30], 1.0).is_empty());
    }

    #[test]
    fn the_report_splits_blur_composite_and_rest() {
        let mut report = Report::default();
        report.add_frame(&[
            SpanTime {
                kind: GpuSpanKind::Frame,
                width: 0,
                height: 0,
                ms: 10.0,
            },
            SpanTime {
                kind: GpuSpanKind::BlurHorizontal,
                width: 270,
                height: 100,
                ms: 1.0,
            },
            SpanTime {
                kind: GpuSpanKind::BlurVertical,
                width: 1080,
                height: 400,
                ms: 2.0,
            },
            SpanTime {
                kind: GpuSpanKind::Composite,
                width: 1080,
                height: 400,
                ms: 3.0,
            },
        ]);
        assert_eq!(report.frames, 1);
        assert!((report.blur_ms() - 3.0).abs() < 1e-9);
        assert!((report.composite_ms() - 3.0).abs() < 1e-9);
        assert!((report.frame_ms - report.spans_ms() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn the_size_report_counts_passes_per_frame() {
        let mut report = SizeReport::new(2);
        report.record(GpuSpanKind::BlurHorizontal, 270, 100);
        report.record(GpuSpanKind::BlurHorizontal, 270, 100);
        report.record(GpuSpanKind::Composite, 1080, 420);
        report.end_frame();
        assert_eq!(report.frames, 1);
        assert_eq!(
            report.buckets.get(&(GpuSpanKind::BlurHorizontal, 270, 100)),
            Some(&2)
        );
        assert_eq!(report.last_frame.len(), 3);
        assert!(report.frame.is_empty());
        report.record(GpuSpanKind::Composite, 1080, 420);
        report.end_frame();
        assert_eq!(report.frames, 0);
        assert!(report.buckets.is_empty());
    }

    #[test]
    fn two_passes_of_one_size_share_a_bucket() {
        let mut report = Report::default();
        let time = SpanTime {
            kind: GpuSpanKind::BlurHorizontal,
            width: 270,
            height: 100,
            ms: 1.5,
        };
        report.add_frame(&[time, time]);
        let bucket = report
            .buckets
            .get(&(GpuSpanKind::BlurHorizontal, 270, 100))
            .copied()
            .expect("the bucket holds both passes");
        assert_eq!(bucket.count, 2);
        assert!((bucket.ms - 3.0).abs() < 1e-9);
    }
}
