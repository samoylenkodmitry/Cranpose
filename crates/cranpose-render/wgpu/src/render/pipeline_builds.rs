use web_time::Instant;

use super::instant_ms;

pub(crate) fn create_render_pipeline_logged<'a>(
    device: &wgpu::Device,
    cache: Option<&'a wgpu::PipelineCache>,
    tag: &str,
    mut descriptor: wgpu::RenderPipelineDescriptor<'a>,
) -> wgpu::RenderPipeline {
    descriptor.cache = cache;
    build_pipeline_logged(tag, || device.create_render_pipeline(&descriptor))
}

pub(crate) fn build_pipeline_logged<T>(tag: &str, build: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let pipeline = build();
    log::info!(
        "[pipeline-create] {tag} {:.1}ms on {}",
        instant_ms(started, Instant::now()),
        std::thread::current().name().unwrap_or("unnamed thread"),
    );
    crate::thread_state::with(|state| {
        if state.off_frame_builds.get() {
            PIPELINES_CREATED_OFF_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            PIPELINES_CREATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            state.thread_builds.set(state.thread_builds.get() + 1);
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    crate::pipeline_disk_cache::note_change();
    pipeline
}

/// Pipelines this process has built on a thread that draws.
///
/// A build runs the backend's shader compiler, and whoever asks for one while
/// drawing waits for it there. A count that grows across an interaction names
/// work a person waited on, whatever the driver's own caches made a compile
/// cost on this machine. Builds handed to [`crate::pipeline_compiler`] are
/// counted apart, by [`pipelines_created_off_frame`]: they cost a frame
/// nothing.
static PIPELINES_CREATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PIPELINES_CREATED_OFF_FRAME: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// The pipelines this thread has built while drawing: a frame that drew
/// with this unchanged built nothing itself.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn thread_builds() -> u64 {
    crate::thread_state::with(|state| state.thread_builds.get())
}

/// Declares that pipelines built on this thread are built away from any
/// frame. The compiler thread says so once, when it starts.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn mark_thread_off_frame() {
    crate::thread_state::with(|state| state.off_frame_builds.set(true));
}

/// The pipelines this process has built on a thread that draws, since it started. A count
/// that grows during an interaction names a shader compile a frame waited for.
pub fn pipelines_created() -> u64 {
    PIPELINES_CREATED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Pipelines built away from every frame, on the compiler thread.
pub fn pipelines_created_off_frame() -> u64 {
    PIPELINES_CREATED_OFF_FRAME.load(std::sync::atomic::Ordering::Relaxed)
}
