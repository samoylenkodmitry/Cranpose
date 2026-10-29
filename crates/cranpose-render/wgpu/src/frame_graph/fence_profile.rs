// Lives in the frame graph module because only that module may finish an
// encoder and submit it.

use web_time::Instant;

const DEFAULT_REPORT_EVERY_FRAMES: u32 = 60;

fn report_every_frames() -> u32 {
    cranpose_core::DIAGNOSTICS
        .gpu_fence_profile
        .parse::<u32>()
        .filter(|frames| *frames > 0)
        .unwrap_or(DEFAULT_REPORT_EVERY_FRAMES)
}

pub(crate) fn enabled() -> bool {
    cranpose_core::DIAGNOSTICS.gpu_fence_profile.is_set()
}

/// `CRANPOSE_GPU_FENCE_PROFILE=frame`: one fence per frame instead of one
/// per pass, so the report is the frame's whole GPU time with the
/// pipeline intact.
fn whole_frame() -> bool {
    cranpose_core::DIAGNOSTICS.gpu_fence_profile.equals("frame")
}

pub(crate) struct Profile {
    current_label: Option<String>,
    totals: Vec<(String, f64, u32)>,
    frames: u32,
    pending: Vec<(Option<String>, wgpu::CommandBuffer)>,
}

impl Profile {
    pub(crate) const fn new() -> Self {
        Self {
            current_label: None,
            totals: Vec::new(),
            frames: 0,
            pending: Vec::new(),
        }
    }
}

pub(super) struct FrameRecording;

impl Drop for FrameRecording {
    fn drop(&mut self) {
        crate::thread_state::with(|state| {
            let mut profile = state.fence_profile.borrow_mut();
            profile.pending.clear();
            profile.current_label = None;
        });
    }
}

fn submit_and_wait(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    commands: wgpu::CommandBuffer,
) -> f64 {
    let start = Instant::now();
    let submission = queue.submit(std::iter::once(commands));
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: None,
    });
    start.elapsed().as_secs_f64() * 1000.0
}

fn new_encoder(device: &wgpu::Device) -> wgpu::CommandEncoder {
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("fence profile split"),
    })
}

/// The pass label extended with its first color target's size, so passes on
/// the full-screen target rank apart from the same pass on a small layer.
pub(crate) fn bucket_label(descriptor: &wgpu::RenderPassDescriptor<'_>) -> String {
    let label = descriptor.label.unwrap_or("<unlabeled>");
    match descriptor
        .color_attachments
        .first()
        .and_then(|attachment| attachment.as_ref())
    {
        Some(attachment) => {
            let texture = attachment.view.texture();
            let load = match attachment.ops.load {
                wgpu::LoadOp::Load => "load",
                wgpu::LoadOp::Clear(_) => "clear",
                wgpu::LoadOp::DontCare(_) => "dontcare",
            };
            format!("{label} {}x{} {load}", texture.width(), texture.height())
        }
        None => label.to_owned(),
    }
}

pub(crate) fn split(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    next_label: Option<&str>,
) {
    crate::thread_state::with(|state| {
        let mut profile = state.fence_profile.borrow_mut();
        if whole_frame() && next_label.is_some() {
            profile
                .current_label
                .get_or_insert_with(|| "frame".to_owned());
            return;
        }
        let finished = std::mem::replace(encoder, new_encoder(device));
        let label = profile.current_label.take();
        profile.pending.push((label, finished.finish()));
        profile.current_label = next_label.map(str::to_owned);
    });
}

/// Charges the frame's last pass and prints the per-label ranking once per
/// report period.
#[expect(clippy::print_stderr, reason = "the report prints when switched on")]
pub(crate) fn end_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    before_passes: Option<wgpu::CommandEncoder>,
) {
    split(device, encoder, None);
    if let Some(before_passes) = before_passes {
        submit_and_wait(device, queue, before_passes.finish());
    }
    crate::thread_state::with(|state| {
        let mut profile = state.fence_profile.borrow_mut();
        let mut pending = std::mem::take(&mut profile.pending);
        for (label, commands) in pending.drain(..) {
            let recorded = submit_and_wait(device, queue, commands);
            let empty = submit_and_wait(device, queue, new_encoder(device).finish());
            let elapsed = (recorded - empty).max(0.0);
            if let Some(label) = label {
                match profile
                    .totals
                    .iter_mut()
                    .find(|(name, _, _)| *name == label)
                {
                    Some(entry) => {
                        entry.1 += elapsed;
                        entry.2 += 1;
                    }
                    None => profile.totals.push((label, elapsed, 1)),
                }
            }
        }
        profile.pending = pending;
        profile.frames += 1;
        if profile.frames < report_every_frames() {
            return;
        }
        let frames = f64::from(profile.frames);
        let mut totals = std::mem::take(&mut profile.totals);
        totals.sort_by(|a, b| b.1.total_cmp(&a.1));
        let total_ms: f64 = totals.iter().map(|(_, ms, _)| ms).sum::<f64>() / frames;
        let mut line = format!(
            "[gpu-fence] frames={} total={total_ms:.2}ms/frame",
            profile.frames
        );
        for (label, ms, count) in &totals {
            use std::fmt::Write;
            let _ = write!(
                line,
                " [{label}]={:.2}ms x{:.1}",
                ms / frames,
                f64::from(*count) / frames
            );
        }
        eprintln!("{line}");
        profile.frames = 0;
    });
}

#[cfg(test)]
#[path = "fence_profile/tests/frame_graph_fence_profile_tests.rs"]
mod tests;
