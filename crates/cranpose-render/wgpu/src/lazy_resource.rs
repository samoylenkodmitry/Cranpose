use std::sync::OnceLock;

use web_time::Instant;

/// Creates an immutable GPU resource on first use and reports its real cost.
pub(crate) struct LazyGpuResource<T> {
    label: &'static str,
    value: OnceLock<T>,
}

impl<T> LazyGpuResource<T> {
    pub(crate) const fn new(label: &'static str) -> Self {
        Self {
            label,
            value: OnceLock::new(),
        }
    }

    pub(crate) fn get_or_init(&self, backend: wgpu::Backend, create: impl FnOnce() -> T) -> &T {
        self.value.get_or_init(|| {
            let started = Instant::now();
            let value = create();
            log::info!(
                "[gpu-pipeline] {:?} {} ready in {:.1} ms",
                backend,
                self.label,
                crate::render::instant_ms(started, Instant::now()),
            );
            value
        })
    }

    pub(crate) fn get(&self) -> Option<&T> {
        self.value.get()
    }

    #[cfg(test)]
    fn initialized(&self) -> bool {
        self.value.get().is_some()
    }
}

/// A render pipeline in its two pass flavors: `flat` for passes without a
/// depth attachment (every pass except the display-clip culled fused pass)
/// and `depth` for that one pass (content tests depth against the
/// visible-region occluder, write off). The depth slot stays uninitialized
/// — truly zero cost — until a cullable visible region actually engages
/// the cull, and the flat slot is created from the identical inputs it
/// always was, so full-region targets render bitwise identically.
pub(crate) struct PassPipeline {
    flat: LazyGpuResource<wgpu::RenderPipeline>,
    depth: LazyGpuResource<wgpu::RenderPipeline>,
}

impl PassPipeline {
    pub(crate) const fn new(flat_label: &'static str, depth_label: &'static str) -> Self {
        Self {
            flat: LazyGpuResource::new(flat_label),
            depth: LazyGpuResource::new(depth_label),
        }
    }

    /// Returns the variant for `depth`, creating it on first use. The
    /// closure receives the variant it must build, so one call site serves
    /// both flavors.
    pub(crate) fn get_or_init(
        &self,
        backend: wgpu::Backend,
        depth: bool,
        create: impl FnOnce(bool) -> wgpu::RenderPipeline,
    ) -> &wgpu::RenderPipeline {
        if depth {
            self.depth.get_or_init(backend, || create(true))
        } else {
            self.flat.get_or_init(backend, || create(false))
        }
    }

    pub(crate) fn get(&self, depth: bool) -> Option<&wgpu::RenderPipeline> {
        if depth {
            self.depth.get()
        } else {
            self.flat.get()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn resource_is_created_only_when_first_requested() {
        let resource = LazyGpuResource::new("test");
        let calls = Cell::new(0);
        assert!(!resource.initialized());

        let first = resource.get_or_init(wgpu::Backend::Gl, || {
            calls.set(calls.get() + 1);
            41
        });
        let second = resource.get_or_init(wgpu::Backend::Gl, || {
            calls.set(calls.get() + 1);
            99
        });

        assert_eq!((*first, *second), (41, 41));
        assert_eq!(calls.get(), 1);
        assert!(resource.initialized());
    }
}
