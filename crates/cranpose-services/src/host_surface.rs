//! The surface the host gives the application, and what the application may
//! ask of it.
//!
//! On the web that is the canvas inside its page; on desktop the window's
//! client area; on mobile the activity's content view. An application that
//! wants to lay out against it — or, on a host that allows it, ask for a
//! different size — reads observable state here instead of reaching for a
//! platform API and a resize callback of its own.

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use cranpose_core::{State, rememberEventStream};

use crate::registry::ServiceRegistry;

/// The size of the host surface, in logical pixels, with the scale the host
/// renders it at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostSurfaceSize {
    /// Logical width.
    pub width: f32,
    /// Logical height.
    pub height: f32,
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

impl Default for HostSurfaceSize {
    fn default() -> Self {
        Self {
            width: 0.0,
            height: 0.0,
            scale: 1.0,
        }
    }
}

impl HostSurfaceSize {
    /// The size in physical pixels.
    pub fn physical(&self) -> (u32, u32) {
        (
            (self.width * self.scale).round().max(0.0) as u32,
            (self.height * self.scale).round().max(0.0) as u32,
        )
    }
}

/// Why a resize request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResizeRefused {
    /// This host does not let an application choose its surface size — a
    /// fullscreen mobile activity, a maximised window, a fixed canvas.
    #[error("this host does not accept surface resize requests")]
    Unsupported,
    /// The host accepted the idea but refused these dimensions.
    #[error("the host refused the requested surface size")]
    Rejected,
}

/// What an application may ask of the host's surface.
///
/// The surface's *size* is not asked of a backend: every host publishes it as
/// it lays the surface out, and [`host_surface_size`] answers from that. What a
/// backend adds is the other direction — whether this host lets the application
/// choose a size, and how to ask for one.
pub trait HostSurface: Send + Sync {
    /// Whether this host accepts resize requests at all.
    fn can_resize(&self) -> bool {
        false
    }

    /// Asks the host for a different surface size.
    ///
    /// Hosts are free to clamp or ignore the request, so the answer is only
    /// "the request was accepted": the size that actually took effect arrives
    /// through the observable state.
    fn request_size(&self, width: f32, height: f32) -> Result<(), ResizeRefused> {
        let _ = (width, height);
        Err(ResizeRefused::Unsupported)
    }
}

/// Shared handle to a [`HostSurface`].
pub type HostSurfaceRef = Arc<dyn HostSurface>;

struct NoHostSurface;

/// A host that owns its own window: a fullscreen activity, a maximised window,
/// a canvas the page sizes.
impl HostSurface for NoHostSurface {}

static PLATFORM_HOST_SURFACE: ServiceRegistry<dyn HostSurface> = ServiceRegistry::new();

/// Installs the platform host surface.
pub fn set_platform_host_surface(surface: HostSurfaceRef) {
    PLATFORM_HOST_SURFACE.set(surface);
}

/// Removes the installed host surface (tests and teardown).
pub fn clear_platform_host_surface() {
    PLATFORM_HOST_SURFACE.clear();
    if let Ok(mut observers) = observers().lock() {
        observers.clear();
    }
    if let Ok(mut last) = last_published().lock() {
        *last = HostSurfaceSize::default();
    }
}

/// Whether this host lets the application choose its surface size.
pub fn host_surface_can_resize() -> bool {
    host_surface().can_resize()
}

/// The installed host surface, or one that accepts no resize requests.
pub fn host_surface() -> HostSurfaceRef {
    PLATFORM_HOST_SURFACE
        .get()
        .unwrap_or_else(|| Arc::new(NoHostSurface))
}

/// The size the host last reported.
///
/// Read from what the host published rather than asked of a backend: every host
/// publishes its surface as it lays it out, and a host that has no resize
/// facility to install a backend for still reports its size. Before the first
/// frame this is the empty surface at scale one — nothing has been measured
/// yet — so a caller that needs the real value observes
/// [`rememberHostSurfaceSize`] rather than sampling once at startup.
pub fn host_surface_size() -> HostSurfaceSize {
    last_published()
        .lock()
        .map(|size| *size)
        .unwrap_or_default()
}

fn last_published() -> &'static Mutex<HostSurfaceSize> {
    static SLOT: OnceLock<Mutex<HostSurfaceSize>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HostSurfaceSize::default()))
}

/// Asks the host for a different surface size.
pub fn request_host_surface_size(width: f32, height: f32) -> Result<(), ResizeRefused> {
    host_surface().request_size(width, height)
}

type Observer = Arc<dyn Fn(HostSurfaceSize) + Send + Sync>;

fn observers() -> &'static Mutex<Vec<(u64, Observer)>> {
    static SLOT: OnceLock<Mutex<Vec<(u64, Observer)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

static NEXT_OBSERVER: AtomicU64 = AtomicU64::new(1);

/// Keeps a host-surface observer registered until it is dropped.
pub struct HostSurfaceObserver {
    id: u64,
}

impl Drop for HostSurfaceObserver {
    fn drop(&mut self) {
        if let Ok(mut observers) = observers().lock() {
            observers.retain(|(id, _)| *id != self.id);
        }
    }
}

/// Registers `observer` for host-surface size changes. Applications collect
/// [`rememberHostSurfaceSize`] instead of calling this.
pub fn observe_host_surface_size(
    observer: impl Fn(HostSurfaceSize) + Send + Sync + 'static,
) -> HostSurfaceObserver {
    let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut observers) = observers().lock() {
        observers.push((id, Arc::new(observer)));
    }
    HostSurfaceObserver { id }
}

/// Publishes a new host-surface size. Platform backends call this whenever the
/// host resizes the surface.
pub fn publish_host_surface_size(size: HostSurfaceSize) {
    if let Ok(mut last) = last_published().lock() {
        if *last == size {
            return;
        }
        *last = size;
    }
    let observers = observers()
        .lock()
        .map(|observers| {
            observers
                .iter()
                .map(|(_, observer)| Arc::clone(observer))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for observer in observers {
        observer(size);
    }
}

/// The host surface's size, observed for as long as this call stays in the
/// composition.
#[allow(non_snake_case)]
#[track_caller]
pub fn rememberHostSurfaceSize() -> State<HostSurfaceSize> {
    let updates = rememberEventStream((), |sender| {
        observe_host_surface_size(move |size| sender.send(size))
    });
    cranpose_core::collectAsState(updates, (), host_surface_size())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedSurface;

    /// A host that owns its own window and answers no to being resized.
    impl HostSurface for FixedSurface {}

    struct ResizableSurface {
        requested: Mutex<Option<(f32, f32)>>,
    }

    impl HostSurface for ResizableSurface {
        fn can_resize(&self) -> bool {
            true
        }

        fn request_size(&self, width: f32, height: f32) -> Result<(), ResizeRefused> {
            if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
                return Err(ResizeRefused::Rejected);
            }
            *self
                .requested
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some((width, height));
            Ok(())
        }
    }

    #[test]
    fn a_host_that_has_not_drawn_yet_reports_an_empty_surface() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_host_surface();
        let size = host_surface_size();
        assert_eq!(size, HostSurfaceSize::default());
        assert_eq!(size.scale, 1.0, "a scale of zero would divide by zero");
        assert!(!host_surface_can_resize());
        assert_eq!(
            request_host_surface_size(320.0, 200.0),
            Err(ResizeRefused::Unsupported)
        );
    }

    #[test]
    fn the_surface_size_is_whatever_the_host_last_published() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_host_surface();
        publish_host_surface_size(HostSurfaceSize {
            width: 640.0,
            height: 480.0,
            scale: 2.0,
        });
        let size = host_surface_size();
        assert_eq!(size.width, 640.0);
        assert_eq!(size.physical(), (1280, 960));
        clear_platform_host_surface();
    }

    #[test]
    fn a_host_that_owns_its_window_refuses_resize_requests() {
        let _guard = crate::registry::test_service_guard();
        set_platform_host_surface(Arc::new(FixedSurface));
        assert!(!host_surface_can_resize());
        assert_eq!(
            request_host_surface_size(320.0, 200.0),
            Err(ResizeRefused::Unsupported)
        );
        clear_platform_host_surface();
    }

    #[test]
    fn a_resizable_surface_takes_the_request_and_refuses_nonsense() {
        let _guard = crate::registry::test_service_guard();
        let surface = Arc::new(ResizableSurface {
            requested: Mutex::new(None),
        });
        set_platform_host_surface(surface.clone());
        assert!(host_surface_can_resize());
        assert_eq!(request_host_surface_size(275.0, 116.0), Ok(()));
        assert_eq!(
            *surface
                .requested
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            Some((275.0, 116.0))
        );
        assert_eq!(
            request_host_surface_size(0.0, 116.0),
            Err(ResizeRefused::Rejected)
        );
        assert_eq!(
            request_host_surface_size(f32::NAN, 116.0),
            Err(ResizeRefused::Rejected)
        );
        clear_platform_host_surface();
    }

    #[test]
    fn observers_see_published_sizes_and_stop_when_dropped() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_host_surface();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let registration = observe_host_surface_size(move |size| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(size)
        });
        let first = HostSurfaceSize {
            width: 100.0,
            height: 50.0,
            scale: 1.0,
        };
        publish_host_surface_size(first);
        // The same size again is not a change, so nothing is told about it.
        publish_host_surface_size(first);
        assert_eq!(
            seen.lock().unwrap_or_else(|e| e.into_inner()).as_slice(),
            [first]
        );
        drop(registration);
        publish_host_surface_size(HostSurfaceSize {
            width: 200.0,
            ..first
        });
        assert_eq!(seen.lock().unwrap_or_else(|e| e.into_inner()).len(), 1);
        clear_platform_host_surface();
    }
}
