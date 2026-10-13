use std::sync::Arc;

use cranpose_core::{CompositionLocalProvider, StaticCompositionLocal, staticCompositionLocalOf};

/// The GPU objects a Cranpose renderer draws with, for code that draws into
/// textures Cranpose shows, such as a game engine rendering into an
/// [`ExternalTexture`](crate::prelude::ExternalTexture).
///
/// Work submitted to [`queue`](Self::queue) before Cranpose draws a frame is
/// finished before that frame samples its results, so a texture drawn on this
/// device is shown without a copy.
#[derive(Clone, Debug)]
pub struct GpuContext {
    /// The instance the adapter came from.
    pub instance: wgpu::Instance,
    /// The adapter the device was created on.
    pub adapter: wgpu::Adapter,
    /// The device Cranpose renders with.
    pub device: Arc<wgpu::Device>,
    /// The queue Cranpose submits its frames to.
    pub queue: Arc<wgpu::Queue>,
}

impl PartialEq for GpuContext {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.device, &other.device) && Arc::ptr_eq(&self.queue, &other.queue)
    }
}

/// What code that renders with Cranpose's device (see [`GpuContext`])
/// needs from it beyond what Cranpose asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuRequirements {
    /// Features enabled where the adapter offers them. Experimental features
    /// stay off.
    pub features: wgpu::Features,
    /// Whether the device takes the adapter's limits instead of wgpu's
    /// defaults.
    pub adapter_limits: bool,
}

impl GpuRequirements {
    /// The features of `adapter` these requirements enable.
    pub fn features_on(&self, adapter: &wgpu::Adapter) -> wgpu::Features {
        self.features & (adapter.features() - wgpu::Features::all_experimental_mask())
    }

    /// The limits a device on `adapter` is created with, where `own` are the
    /// limits the platform asks for by itself.
    pub fn limits_on(&self, adapter: &wgpu::Adapter, own: wgpu::Limits) -> wgpu::Limits {
        if self.adapter_limits {
            adapter.limits()
        } else {
            own
        }
    }
}

fn local_gpu_context() -> StaticCompositionLocal<Option<GpuContext>> {
    crate::thread_state::with(|state| {
        state
            .gpu_context_local
            .get_or_init(|| staticCompositionLocalOf(|| None))
            .clone()
    })
}

/// The GPU context of the window this composition draws into. `None` on
/// platforms that do not share their device yet.
pub fn gpu_context() -> Option<GpuContext> {
    local_gpu_context().current()
}

pub(crate) fn provide_gpu_context(
    context: GpuContext,
    mut content: impl FnMut() + 'static,
) -> impl FnMut() + 'static {
    move || {
        CompositionLocalProvider(
            [local_gpu_context().provides(Some(context.clone()))],
            &mut content,
        );
    }
}
