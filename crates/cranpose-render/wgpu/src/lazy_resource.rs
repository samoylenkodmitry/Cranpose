use std::sync::OnceLock;

use web_time::Instant;

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
