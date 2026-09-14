use std::sync::{Arc, OnceLock};

use web_time::Instant;

use crate::pipeline_compiler::{CompilerSend, CompilerSync, PipelineCompiler};

/// A GPU resource created once, by whichever thread asks first: a warm-up
/// queued on the background compiler, or the frame that needs it. A frame
/// arriving while the warm-up is under way waits for that one creation
/// rather than starting a second.
pub(crate) struct LazyGpuResource<T> {
    label: &'static str,
    value: Arc<OnceLock<T>>,
}

impl<T> Clone for LazyGpuResource<T> {
    fn clone(&self) -> Self {
        Self {
            label: self.label,
            value: Arc::clone(&self.value),
        }
    }
}

impl<T> LazyGpuResource<T> {
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            label,
            value: Arc::new(OnceLock::new()),
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
}

impl<T: CompilerSend + CompilerSync + 'static> LazyGpuResource<T> {
    pub(crate) fn warm(
        &self,
        compiler: &PipelineCompiler,
        backend: wgpu::Backend,
        create: impl FnOnce() -> T + CompilerSend + 'static,
    ) {
        let resource = self.clone();
        compiler.enqueue(move || {
            resource.get_or_init(backend, create);
        });
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
        assert!(resource.get().is_none());

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
        assert!(resource.get().is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_request_during_the_warm_up_waits_for_it_instead_of_creating_twice() {
        use std::{sync::mpsc, time::Duration};

        let compiler = PipelineCompiler::spawn();
        let resource = LazyGpuResource::new("warmed");
        let (started, observed) = mpsc::channel();
        let (release, blocked) = mpsc::channel::<()>();
        resource.warm(&compiler, wgpu::Backend::Gl, move || {
            started.send(()).unwrap();
            blocked.recv().unwrap();
            7
        });
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(resource.get().is_none(), "the warm-up is still running");
        let waiter = {
            let resource = resource.clone();
            std::thread::spawn(move || {
                *resource.get_or_init(wgpu::Backend::Gl, || {
                    panic!("a second creation ran beside the warm-up")
                })
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !waiter.is_finished(),
            "the request must wait for the warm-up"
        );
        release.send(()).unwrap();
        assert_eq!(waiter.join().unwrap(), 7);
        assert_eq!(resource.get(), Some(&7));
    }

    #[test]
    fn an_inactive_compiler_leaves_the_resource_to_its_first_use() {
        let compiler = PipelineCompiler::inactive();
        let resource = LazyGpuResource::new("lazy");
        resource.warm(&compiler, wgpu::Backend::Gl, || 3);
        assert!(resource.get().is_none());
        assert_eq!(*resource.get_or_init(wgpu::Backend::Gl, || 4), 4);
    }
}
