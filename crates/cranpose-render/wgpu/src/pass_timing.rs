use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

const QUERY_CAPACITY: u32 = 512;

const READBACK_SLOTS: usize = 4;

const SLOT_FREE: u8 = 0;
const SLOT_PENDING: u8 = 1;
const SLOT_MAPPED: u8 = 2;
const SLOT_FAILED: u8 = 3;

const PRINT_CADENCE_FRAMES: u64 = 60;

pub(crate) fn pass_timing_requested() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_GPU_PASS_TIMING")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// One label's aggregate inside the current print window.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuPassTimingEntry {
    pub label: String,
    pub total_ms: f64,
    pub passes: u64,
}

/// GPU time by pass label, aggregated since the last `[GPU-PASS]` print.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuPassTimingReport {
    /// Frames whose timestamps have been read back into this window.
    pub frames: u32,
    /// Total GPU busy span — first pass begin to last pass end, summed over
    /// the window's frames. Per-label times are stage-boundary occupancy
    /// windows that overlap on pipelined GPUs and can sum past the frame;
    /// this span is the frame-level wall number they cannot give.
    pub span_ms: f64,
    /// Entries sorted by descending GPU time; zero-time labels are omitted.
    pub entries: Vec<GpuPassTimingEntry>,
}

#[derive(Clone, Copy, Default)]
struct LabelTotal {
    nanoseconds: u64,
    passes: u64,
}

struct ReadbackSlot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    passes: RefCell<Vec<(u16, u32)>>,
}

pub(crate) struct PassTimer {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    period_ns: f32,
    cursor: Cell<u32>,
    frame_passes: RefCell<Vec<(u16, u32)>>,
    labels: RefCell<Vec<String>>,
    totals: RefCell<Vec<LabelTotal>>,
    slots: Vec<ReadbackSlot>,
    frame_index: Cell<u64>,
    frames_harvested: Cell<u32>,
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
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("Pass Timing Query Set"),
            ty: wgpu::QueryType::Timestamp,
            count: QUERY_CAPACITY,
        });
        let buffer_size = u64::from(QUERY_CAPACITY) * u64::from(wgpu::QUERY_SIZE);
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pass Timing Resolve Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..READBACK_SLOTS)
            .map(|_| ReadbackSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Pass Timing Readback Buffer"),
                    size: buffer_size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(SLOT_FREE)),
                passes: RefCell::new(Vec::new()),
            })
            .collect();
        Some(Self {
            query_set,
            resolve_buffer,
            period_ns: queue.get_timestamp_period(),
            cursor: Cell::new(0),
            frame_passes: RefCell::new(Vec::new()),
            labels: RefCell::new(Vec::new()),
            totals: RefCell::new(Vec::new()),
            slots,
            frame_index: Cell::new(0),
            frames_harvested: Cell::new(0),
            span_nanoseconds: Cell::new(0),
            dropped_passes: Cell::new(0),
            dropped_frames: Cell::new(0),
        })
    }

    pub(crate) fn query_set(&self) -> &wgpu::QuerySet {
        &self.query_set
    }

    pub(crate) fn begin_pass(&self, label: &str) -> Option<(u32, u32)> {
        let begin = self.cursor.get();
        if begin + 2 > QUERY_CAPACITY {
            self.dropped_passes.set(self.dropped_passes.get() + 1);
            return None;
        }
        self.cursor.set(begin + 2);
        let label_id = self.intern(label);
        self.frame_passes.borrow_mut().push((label_id, begin));
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
                    {
                        let mapped = slot.buffer.slice(..).get_mapped_range();
                        let span = accumulate_frame(
                            &mut self.totals.borrow_mut(),
                            &slot.passes.borrow(),
                            &mapped,
                            self.period_ns,
                        );
                        self.span_nanoseconds
                            .set(self.span_nanoseconds.get().saturating_add(span));
                    }
                    slot.buffer.unmap();
                    slot.passes.borrow_mut().clear();
                    slot.state.store(SLOT_FREE, Ordering::Release);
                    self.frames_harvested.set(self.frames_harvested.get() + 1);
                }
                SLOT_FAILED => {
                    slot.passes.borrow_mut().clear();
                    slot.state.store(SLOT_FREE, Ordering::Release);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn frame_resolve(&self) -> Option<PendingResolve<'_>> {
        let used = self.cursor.get();
        if used == 0 {
            return None;
        }
        let Some(slot_index) = self
            .slots
            .iter()
            .position(|slot| slot.state.load(Ordering::Acquire) == SLOT_FREE)
        else {
            self.dropped_frames.set(self.dropped_frames.get() + 1);
            return None;
        };
        Some(PendingResolve {
            timer: self,
            slot_index,
            used,
        })
    }

    pub(crate) fn finish_frame(&self) {
        self.cursor.set(0);
        self.frame_passes.borrow_mut().clear();
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
            span_ms: self.span_nanoseconds.get() as f64 / 1_000_000.0,
            entries,
        }
    }

    fn print_and_reset_window(&self, frame: u64) {
        let report = self.report();
        if report.frames > 0 {
            let frames = f64::from(report.frames);
            let total_ms: f64 = report.entries.iter().map(|entry| entry.total_ms).sum();
            let mut line = format!(
                "[GPU-PASS f#{frame}] frames={} span={:.2}ms/frame occupancy={:.2}ms/frame",
                report.frames,
                report.span_ms / frames,
                total_ms / frames,
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
        self.span_nanoseconds.set(0);
        self.dropped_passes.set(0);
        self.dropped_frames.set(0);
    }
}

pub(crate) struct PendingResolve<'timer> {
    timer: &'timer PassTimer,
    slot_index: usize,
    used: u32,
}

impl PendingResolve<'_> {
    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let slot = &self.timer.slots[self.slot_index];
        encoder.resolve_query_set(
            &self.timer.query_set,
            0..self.used,
            &self.timer.resolve_buffer,
            0,
        );
        encoder.copy_buffer_to_buffer(
            &self.timer.resolve_buffer,
            0,
            &slot.buffer,
            0,
            u64::from(self.used) * u64::from(wgpu::QUERY_SIZE),
        );
    }

    pub(crate) fn arm_readback(self) {
        let slot = &self.timer.slots[self.slot_index];
        slot.passes
            .borrow_mut()
            .clone_from(&self.timer.frame_passes.borrow());
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
) -> u64 {
    let read_tick = |index: u32| -> Option<u64> {
        let offset = index as usize * 8;
        let bytes = mapped.get(offset..offset + 8)?;
        Some(u64::from_le_bytes(bytes.try_into().expect("8-byte slice")))
    };
    let mut first_begin = u64::MAX;
    let mut last_end = 0u64;
    for &(label_id, begin_index) in passes {
        let Some(total) = totals.get_mut(usize::from(label_id)) else {
            continue;
        };
        let (Some(begin), Some(end)) = (read_tick(begin_index), read_tick(begin_index + 1)) else {
            continue;
        };
        if end < begin {
            continue;
        }
        first_begin = first_begin.min(begin);
        last_end = last_end.max(end);
        total.nanoseconds = total
            .nanoseconds
            .saturating_add(((end - begin) as f64 * f64::from(period_ns)) as u64);
        total.passes += 1;
    }
    if last_end <= first_begin {
        return 0;
    }
    ((last_end - first_begin) as f64 * f64::from(period_ns)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(values: &[u64]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn accumulate_frame_attributes_ticks_by_label() {
        let mut totals = vec![LabelTotal::default(); 2];
        let mapped = ticks(&[1_000, 1_100, 1_100, 1_150, 1_150, 1_160]);
        accumulate_frame(&mut totals, &[(0, 0), (1, 2), (0, 4)], &mapped, 2.0);
        assert_eq!(totals[0].nanoseconds, 220);
        assert_eq!(totals[0].passes, 2);
        assert_eq!(totals[1].nanoseconds, 100);
        assert_eq!(totals[1].passes, 1);
    }

    #[test]
    fn accumulate_frame_skips_backwards_and_out_of_range_pairs() {
        let mut totals = vec![LabelTotal::default(); 1];
        let mapped = ticks(&[500, 400]);
        accumulate_frame(&mut totals, &[(0, 0)], &mapped, 1.0);
        assert_eq!(totals[0].passes, 0, "an end before its begin is skipped");

        accumulate_frame(&mut totals, &[(0, 6)], &mapped, 1.0);
        assert_eq!(totals[0].passes, 0, "indices past the mapping are skipped");

        let mapped = ticks(&[100, 250]);
        accumulate_frame(&mut totals, &[(9, 0)], &mapped, 1.0);
        assert_eq!(totals[0].passes, 0, "an unknown label id is skipped");
    }
}
