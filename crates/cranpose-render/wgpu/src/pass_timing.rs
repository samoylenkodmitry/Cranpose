//! Per-pass GPU time attribution.
//!
//! The counters in [`crate::gpu_stats`] say how many passes ran and how many
//! pixels they filled; they cannot say which pass the GPU spent its
//! milliseconds in, and fill-pixel proxies mislead on tile-based GPUs where a
//! megapixel of flat translucent fill costs a fraction of a megapixel of
//! blur. This module answers the time question directly: with
//! `CRANPOSE_GPU_PASS_TIMING=1` (`debug.cranpose.pass_timing` over adb),
//! every render pass writes begin/end timestamps
//! ([`wgpu::Features::TIMESTAMP_QUERY`]), the deltas are aggregated by pass
//! label, and a `[GPU-PASS]` line prints every 60 frames beside the
//! `[GPU f#]` counter line.
//!
//! One caveat travels with the numbers: GPUs overlap adjacent passes — a
//! tiler shades one pass's fragments while binning the next — so per-pass
//! times measure occupancy, not exclusive wall time, and can sum past the
//! frame. They rank passes and size their work; frame-level wall time still
//! comes from the frame telemetry.

use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

/// Two timestamps per pass; 256 passes a frame is an order of magnitude above
/// any frame the counters have recorded. Passes beyond it go untimed and are
/// reported as `dropped=`.
const QUERY_CAPACITY: u32 = 512;

/// Readback buffers cycling through frames in flight. With triple buffering a
/// resolve is mapped well within three frames; a slot shortage means the GPU
/// stalled, and the frame is dropped from the aggregate rather than waited on.
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
    /// The `(label id, begin query index)` pairs of the frame resolved into
    /// `buffer`, kept until the mapping is read back.
    passes: RefCell<Vec<(u16, u32)>>,
}

pub(crate) struct PassTimer {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    /// Nanoseconds per timestamp tick, from [`wgpu::Queue::get_timestamp_period`].
    period_ns: f32,
    /// Next free query index this frame; each pass takes two.
    cursor: Cell<u32>,
    frame_passes: RefCell<Vec<(u16, u32)>>,
    /// Interned pass labels; `totals` is parallel to it.
    labels: RefCell<Vec<String>>,
    totals: RefCell<Vec<LabelTotal>>,
    slots: Vec<ReadbackSlot>,
    frame_index: Cell<u64>,
    frames_harvested: Cell<u32>,
    dropped_passes: Cell<u64>,
    dropped_frames: Cell<u64>,
}

impl PassTimer {
    /// The timer for a device that granted [`wgpu::Features::TIMESTAMP_QUERY`];
    /// `None` — with a one-line notice, since the caller asked to profile —
    /// when the adapter cannot time passes.
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
            dropped_passes: Cell::new(0),
            dropped_frames: Cell::new(0),
        })
    }

    pub(crate) fn query_set(&self) -> &wgpu::QuerySet {
        &self.query_set
    }

    /// Reserves the `(begin, end)` query indices for one pass, or `None` when
    /// the frame already timed [`QUERY_CAPACITY`]`/2` passes.
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

    /// Folds every completed readback into the label totals, freeing its
    /// slot. Call once per frame, after a non-blocking device poll gave the
    /// mapping callbacks a chance to fire.
    pub(crate) fn harvest_completed(&self) {
        for slot in &self.slots {
            match slot.state.load(Ordering::Acquire) {
                SLOT_MAPPED => {
                    {
                        let mapped = slot.buffer.slice(..).get_mapped_range();
                        accumulate_frame(
                            &mut self.totals.borrow_mut(),
                            &slot.passes.borrow(),
                            &mapped,
                            self.period_ns,
                        );
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

    /// The frame's resolve work — which queries to copy into which readback
    /// slot — or `None` when nothing was timed or every slot is still in
    /// flight (that frame is counted dropped rather than waited on).
    ///
    /// Encoding and submitting the resolve stay with the frame-graph
    /// executor: it is the sole owner of command encoders and submits, and
    /// the render-contract suite holds that boundary.
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

    /// Resets the per-frame query cursor and prints the aggregate on the
    /// [`PRINT_CADENCE_FRAMES`] cadence. The frame's last timing call.
    pub(crate) fn finish_frame(&self) {
        self.cursor.set(0);
        self.frame_passes.borrow_mut().clear();
        let frame = self.frame_index.get() + 1;
        self.frame_index.set(frame);
        if frame.is_multiple_of(PRINT_CADENCE_FRAMES) {
            self.print_and_reset_window(frame);
        }
    }

    /// The current window's aggregate, for harnesses and tests; printing
    /// resets it on its own cadence.
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
            entries,
        }
    }

    fn print_and_reset_window(&self, frame: u64) {
        let report = self.report();
        if report.frames > 0 {
            let frames = f64::from(report.frames);
            let total_ms: f64 = report.entries.iter().map(|entry| entry.total_ms).sum();
            let mut line = format!(
                "[GPU-PASS f#{frame}] frames={} gpu={:.2}ms/frame",
                report.frames,
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
        self.dropped_passes.set(0);
        self.dropped_frames.set(0);
    }
}

/// One frame's resolve, handed to the frame-graph executor to encode and
/// submit: [`Self::encode`] onto the executor's encoder, then
/// [`Self::arm_readback`] once that encoder is submitted.
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

    /// Hands the frame's pass list to the slot and requests the mapping.
    /// Only valid after the encoded resolve was submitted — a mapping
    /// requested before the copy is enqueued would race it.
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

/// Begins `descriptor`'s pass on `encoder`, timing it when a timer is on.
///
/// The single choke point behind
/// [`FrameCommandRecorder::begin_timed_render_pass`][crate::frame_graph::FrameCommandRecorder::begin_timed_render_pass]:
/// both recorder impls split their own fields into this call, which keeps the
/// query-set borrow and the encoder borrow disjoint.
pub(crate) fn begin_timed_render_pass<'encoder>(
    pass_timer: Option<&PassTimer>,
    encoder: &'encoder mut wgpu::CommandEncoder,
    descriptor: &wgpu::RenderPassDescriptor<'_>,
) -> wgpu::RenderPass<'encoder> {
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

/// Folds one frame's resolved timestamps into the label totals.
///
/// `mapped` holds little-endian `u64` ticks, two per recorded pass; a pair
/// whose end precedes its begin (a reset counter mid-frame) is skipped.
fn accumulate_frame(
    totals: &mut [LabelTotal],
    passes: &[(u16, u32)],
    mapped: &[u8],
    period_ns: f32,
) {
    let read_tick = |index: u32| -> Option<u64> {
        let offset = index as usize * 8;
        let bytes = mapped.get(offset..offset + 8)?;
        Some(u64::from_le_bytes(bytes.try_into().expect("8-byte slice")))
    };
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
        total.nanoseconds = total
            .nanoseconds
            .saturating_add(((end - begin) as f64 * f64::from(period_ns)) as u64);
        total.passes += 1;
    }
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
        // Label 0 at queries 0/1 (100 ticks), label 1 at 2/3 (50), label 0
        // again at 4/5 (10).
        let mapped = ticks(&[1_000, 1_100, 1_100, 1_150, 1_150, 1_160]);
        accumulate_frame(
            &mut totals,
            &[(0, 0), (1, 2), (0, 4)],
            &mapped,
            2.0, // 2ns per tick
        );
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
