use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use crate::debug_toggles::DebugToggle;

const QUERY_CAPACITY: u32 = 512;

const READBACK_SLOTS: usize = 4;

const SLOT_FREE: u8 = 0;
const SLOT_PENDING: u8 = 1;
const SLOT_MAPPED: u8 = 2;
const SLOT_FAILED: u8 = 3;
const SLOT_RECORDING: u8 = 4;
const SLOT_READY: u8 = 5;
const SLOT_RESOLVING: u8 = 6;

const PRINT_CADENCE_FRAMES: u64 = 60;

static PASS_TIMING: DebugToggle = DebugToggle::new("CRANPOSE_GPU_PASS_TIMING");

pub(crate) fn pass_timing_requested() -> bool {
    PASS_TIMING.flag()
}

/// One label's aggregate inside the current print window.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuPassTimingEntry {
    pub label: String,
    pub total_ms: f64,
    pub passes: u64,
}

/// GPU time by pass label, aggregated since the last `[GPU-PASS]` print.
/// Check the invalid and dropped counters before comparing workloads; rejected
/// frames contribute neither time nor passes to this report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuPassTimingReport {
    /// Frames whose complete, valid timestamps have been read back into this window.
    pub frames: u32,
    /// Frames rejected because their timestamps were incomplete or invalid.
    pub invalid_frames: u32,
    /// Frames omitted because no query slot was free or a readback failed.
    pub dropped_frames: u64,
    /// Passes omitted because the frame exceeded the query budget.
    pub dropped_passes: u64,
    /// Total GPU span — earliest pass begin to latest pass end, summed over
    /// the window's frames. Per-label times are stage-boundary occupancy
    /// windows that overlap on pipelined GPUs and can sum past the frame;
    /// this span is the frame-level wall number they cannot give.
    pub span_ms: f64,
    /// Entries sorted by descending GPU time.
    pub entries: Vec<GpuPassTimingEntry>,
}

#[derive(Clone, Copy, Default)]
struct LabelTotal {
    nanoseconds: u64,
    passes: u64,
}

struct ReadbackSlot {
    query_set: wgpu::QuerySet,
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    passes: RefCell<Vec<(u16, u32)>>,
    incomplete: Cell<bool>,
}

pub(crate) struct PassTimer {
    recording_slot: Cell<Option<usize>>,
    resolve_buffer: wgpu::Buffer,
    period_ns: f32,
    cursor: Cell<u32>,
    labels: RefCell<Vec<String>>,
    totals: RefCell<Vec<LabelTotal>>,
    slots: Vec<ReadbackSlot>,
    frame_index: Cell<u64>,
    frames_harvested: Cell<u32>,
    invalid_frames: Cell<u32>,
    span_nanoseconds: Cell<u64>,
    dropped_passes: Cell<u64>,
    dropped_frames: Cell<u64>,
}

impl PassTimer {
    pub(crate) fn for_device(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            eprintln!(
                "[GPU-PASS] CRANPOSE_GPU_PASS_TIMING is set but the adapter lacks TIMESTAMP_QUERY; passes will not be timed"
            );
            return None;
        }
        let buffer_size = u64::from(QUERY_CAPACITY) * u64::from(wgpu::QUERY_SIZE);
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pass Timing Resolve Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..READBACK_SLOTS)
            .map(|_| ReadbackSlot {
                query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("Pass Timing Query Set"),
                    ty: wgpu::QueryType::Timestamp,
                    count: QUERY_CAPACITY,
                }),
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Pass Timing Readback Buffer"),
                    size: buffer_size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(SLOT_FREE)),
                passes: RefCell::new(Vec::new()),
                incomplete: Cell::new(false),
            })
            .collect();
        Some(Self {
            recording_slot: Cell::new(None),
            resolve_buffer,
            period_ns: queue.get_timestamp_period(),
            cursor: Cell::new(0),
            labels: RefCell::new(Vec::new()),
            totals: RefCell::new(Vec::new()),
            slots,
            frame_index: Cell::new(0),
            frames_harvested: Cell::new(0),
            invalid_frames: Cell::new(0),
            span_nanoseconds: Cell::new(0),
            dropped_passes: Cell::new(0),
            dropped_frames: Cell::new(0),
        })
    }

    pub(crate) fn query_set(&self) -> &wgpu::QuerySet {
        &self.slots[self
            .recording_slot
            .get()
            .expect("timed pass has a query slot")]
        .query_set
    }

    pub(crate) fn begin_pass(&self, label: &str) -> Option<(u32, u32)> {
        let begin = self.cursor.get();
        if begin == 0 {
            let Some(slot_index) = self
                .slots
                .iter()
                .position(|slot| slot.state.load(Ordering::Acquire) == SLOT_FREE)
            else {
                self.dropped_frames.set(self.dropped_frames.get() + 1);
                self.cursor.set(QUERY_CAPACITY);
                return None;
            };
            self.recording_slot.set(Some(slot_index));
            self.slots[slot_index]
                .state
                .store(SLOT_RECORDING, Ordering::Release);
            self.slots[slot_index].incomplete.set(false);
        }
        if begin + 2 > QUERY_CAPACITY {
            if let Some(slot_index) = self.recording_slot.get() {
                self.dropped_passes.set(self.dropped_passes.get() + 1);
                self.slots[slot_index].incomplete.set(true);
            }
            return None;
        }
        self.cursor.set(begin + 2);
        let label_id = self.intern(label);
        self.slots[self
            .recording_slot
            .get()
            .expect("timed pass has a query slot")]
        .passes
        .borrow_mut()
        .push((label_id, begin));
        Some((begin, begin + 1))
    }

    fn intern(&self, label: &str) -> u16 {
        let mut labels = self.labels.borrow_mut();
        if let Some(id) = labels.iter().position(|known| known == label) {
            return id as u16;
        }
        labels.push(label.to_string());
        self.totals.borrow_mut().push(LabelTotal::default());
        (labels.len() - 1) as u16
    }

    pub(crate) fn harvest_completed(&self) {
        for slot in &self.slots {
            match slot.state.load(Ordering::Acquire) {
                SLOT_MAPPED => {
                    match slot.buffer.slice(..).get_mapped_range() {
                        Ok(mapped) => {
                            let span = (!slot.incomplete.get())
                                .then(|| {
                                    accumulate_frame(
                                        &mut self.totals.borrow_mut(),
                                        &slot.passes.borrow(),
                                        &mapped,
                                        self.period_ns,
                                    )
                                })
                                .flatten();
                            if let Some(span) = span {
                                self.span_nanoseconds
                                    .set(self.span_nanoseconds.get().saturating_add(span));
                                self.frames_harvested.set(self.frames_harvested.get() + 1);
                            } else {
                                self.invalid_frames.set(self.invalid_frames.get() + 1);
                            }
                        }
                        Err(error) => {
                            log::debug!("pass timings could not be read: {error}");
                            self.dropped_frames.set(self.dropped_frames.get() + 1);
                        }
                    }
                    slot.buffer.unmap();
                    slot.passes.borrow_mut().clear();
                    slot.state.store(SLOT_FREE, Ordering::Release);
                }
                SLOT_FAILED => {
                    self.dropped_frames.set(self.dropped_frames.get() + 1);
                    slot.passes.borrow_mut().clear();
                    slot.state.store(SLOT_FREE, Ordering::Release);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn frame_submitted(&self, queue: &wgpu::Queue) {
        let Some(slot_index) = self.recording_slot.get() else {
            return;
        };
        let slot = &self.slots[slot_index];
        slot.state.store(SLOT_PENDING, Ordering::Release);
        let state = Arc::clone(&slot.state);
        queue.on_submitted_work_done(move || state.store(SLOT_READY, Ordering::Release));
    }

    pub(crate) fn has_completed_queries(&self) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.state.load(Ordering::Acquire) == SLOT_READY)
    }

    pub(crate) fn resolve_completed_queries(&self, encoder: &mut wgpu::CommandEncoder) {
        for slot in &self.slots {
            if slot.state.load(Ordering::Acquire) != SLOT_READY {
                continue;
            }
            let used = slot
                .passes
                .borrow()
                .last()
                .expect("submitted timed passes")
                .1
                + 2;
            encoder.resolve_query_set(&slot.query_set, 0..used, &self.resolve_buffer, 0);
            encoder.copy_buffer_to_buffer(
                &self.resolve_buffer,
                0,
                &slot.buffer,
                0,
                u64::from(used) * u64::from(wgpu::QUERY_SIZE),
            );
            slot.state.store(SLOT_RESOLVING, Ordering::Release);
        }
    }

    pub(crate) fn map_resolved_queries(&self) {
        for slot in &self.slots {
            if slot.state.load(Ordering::Acquire) != SLOT_RESOLVING {
                continue;
            }
            slot.state.store(SLOT_PENDING, Ordering::Release);
            let state = Arc::clone(&slot.state);
            slot.buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let outcome = if result.is_ok() {
                        SLOT_MAPPED
                    } else {
                        SLOT_FAILED
                    };
                    state.store(outcome, Ordering::Release);
                });
        }
    }

    pub(crate) fn finish_frame(&self) {
        if let Some(slot_index) = self.recording_slot.take() {
            let slot = &self.slots[slot_index];
            if slot.state.load(Ordering::Acquire) == SLOT_RECORDING {
                slot.passes.borrow_mut().clear();
                slot.state.store(SLOT_FREE, Ordering::Release);
            }
        }
        self.cursor.set(0);
        let frame = self.frame_index.get() + 1;
        self.frame_index.set(frame);
        if frame.is_multiple_of(PRINT_CADENCE_FRAMES) {
            self.print_and_reset_window(frame);
        }
    }

    pub(crate) fn report(&self) -> GpuPassTimingReport {
        let labels = self.labels.borrow();
        let totals = self.totals.borrow();
        let mut entries: Vec<GpuPassTimingEntry> = labels
            .iter()
            .zip(totals.iter())
            .filter(|(_, total)| total.passes > 0)
            .map(|(label, total)| GpuPassTimingEntry {
                label: label.clone(),
                total_ms: total.nanoseconds as f64 / 1_000_000.0,
                passes: total.passes,
            })
            .collect();
        entries.sort_by(|a, b| b.total_ms.total_cmp(&a.total_ms));
        GpuPassTimingReport {
            frames: self.frames_harvested.get(),
            invalid_frames: self.invalid_frames.get(),
            dropped_frames: self.dropped_frames.get(),
            dropped_passes: self.dropped_passes.get(),
            span_ms: self.span_nanoseconds.get() as f64 / 1_000_000.0,
            entries,
        }
    }

    fn print_and_reset_window(&self, frame: u64) {
        let report = self.report();
        if report.frames > 0 || report.invalid_frames > 0 || report.dropped_frames > 0 {
            let frames = f64::from(report.frames.max(1));
            let total_ms: f64 = report.entries.iter().map(|entry| entry.total_ms).sum();
            let mut line = format!(
                "[GPU-PASS f#{frame}] frames={} span={:.2}ms/frame occupancy={:.2}ms/frame invalid_frames={}",
                report.frames,
                report.span_ms / frames,
                total_ms / frames,
                report.invalid_frames,
            );
            for entry in &report.entries {
                line.push_str(&format!(
                    " | {} {:.2}ms x{:.1}",
                    entry.label,
                    entry.total_ms / frames,
                    entry.passes as f64 / frames,
                ));
            }
            if self.dropped_passes.get() > 0 || self.dropped_frames.get() > 0 {
                line.push_str(&format!(
                    " | dropped: passes={} frames={}",
                    self.dropped_passes.get(),
                    self.dropped_frames.get(),
                ));
            }
            eprintln!("{line}");
        }
        for total in self.totals.borrow_mut().iter_mut() {
            *total = LabelTotal::default();
        }
        self.frames_harvested.set(0);
        self.invalid_frames.set(0);
        self.span_nanoseconds.set(0);
        self.dropped_passes.set(0);
        self.dropped_frames.set(0);
    }
}

pub(crate) fn begin_timed_render_pass<'encoder>(
    pass_timer: Option<&PassTimer>,
    encoder: &'encoder mut wgpu::CommandEncoder,
    descriptor: &wgpu::RenderPassDescriptor<'_>,
) -> wgpu::RenderPass<'encoder> {
    crate::frame_graph::note_render_pass(descriptor);
    let timing = pass_timer.and_then(|timer| {
        timer
            .begin_pass(descriptor.label.unwrap_or("<unlabeled pass>"))
            .map(|(begin, end)| (timer, begin, end))
    });
    let timestamp_writes = timing.map(|(timer, begin, end)| wgpu::RenderPassTimestampWrites {
        query_set: timer.query_set(),
        beginning_of_pass_write_index: Some(begin),
        end_of_pass_write_index: Some(end),
    });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        timestamp_writes,
        ..descriptor.clone()
    })
}

fn accumulate_frame(
    totals: &mut [LabelTotal],
    passes: &[(u16, u32)],
    mapped: &[u8],
    period_ns: f32,
) -> Option<u64> {
    let read_tick = |index: u32| -> Option<u64> {
        let offset = index as usize * 8;
        let bytes = mapped.get(offset..offset + 8)?;
        let value = u64::from_le_bytes(bytes.try_into().expect("8-byte slice"));
        (value != 0 && value != u64::MAX).then_some(value)
    };
    let mut first_begin = u64::MAX;
    let mut last_end = 0u64;
    for &(label_id, begin_index) in passes {
        totals.get(usize::from(label_id))?;
        let (begin, end) = (read_tick(begin_index)?, read_tick(begin_index + 1)?);
        if end < begin {
            return None;
        }
        first_begin = first_begin.min(begin);
        last_end = last_end.max(end);
    }
    if passes.is_empty() {
        return None;
    }
    for &(label_id, begin_index) in passes {
        let total = &mut totals[usize::from(label_id)];
        let begin = read_tick(begin_index).expect("validated timestamp");
        let end = read_tick(begin_index + 1).expect("validated timestamp");
        total.nanoseconds = total
            .nanoseconds
            .saturating_add(((end - begin) as f64 * f64::from(period_ns)) as u64);
        total.passes += 1;
    }
    Some(((last_end - first_begin) as f64 * f64::from(period_ns)) as u64)
}

#[cfg(test)]
#[path = "tests/pass_timing_tests.rs"]
mod tests;
