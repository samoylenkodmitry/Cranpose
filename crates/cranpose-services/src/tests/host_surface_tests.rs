use std::sync::PoisonError;

use super::*;

struct FixedSurface;

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
            .unwrap_or_else(PoisonError::into_inner) = Some((width, height));
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
            .unwrap_or_else(PoisonError::into_inner),
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
            .unwrap_or_else(PoisonError::into_inner)
            .push(size);
    });
    let first = HostSurfaceSize {
        width: 100.0,
        height: 50.0,
        scale: 1.0,
    };
    publish_host_surface_size(first);
    publish_host_surface_size(first);
    assert_eq!(
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        [first]
    );
    drop(registration);
    publish_host_surface_size(HostSurfaceSize {
        width: 200.0,
        ..first
    });
    assert_eq!(seen.lock().unwrap_or_else(PoisonError::into_inner).len(), 1);
    clear_platform_host_surface();
}
