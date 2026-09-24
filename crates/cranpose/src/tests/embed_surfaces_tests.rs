use super::*;
use crate::{WindowConfig, native_window::NativeWindowOptions};

fn options(config: WindowConfig) -> NativeWindowOptions {
    config.into_parts().options
}

#[test]
fn a_decorated_window_asks_for_decorations_and_focus() {
    let spec = window_spec(7, &options(WindowConfig::new("Main", 320.0, 240.0)));
    assert_eq!(spec.surface, 7);
    assert_eq!(spec.title, "Main");
    assert_eq!((spec.width, spec.height), (320.0, 240.0));
    assert_eq!(spec.position, None);
    assert!(!spec.relative_to_host);
    assert!(spec.flags & WINDOW_DECORATED != 0);
    assert!(spec.flags & WINDOW_RESIZABLE != 0);
    assert!(spec.flags & WINDOW_TAKES_FOCUS != 0);
    assert!(spec.flags & WINDOW_TRANSPARENT == 0);
}

#[test]
fn a_borderless_floating_window_carries_its_shape_flags() {
    let config = WindowConfig::borderless("Orb", 90.0, 90.0)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_focus(WindowFocus::Never)
        .with_host_window_position(12.0, 34.0);
    let spec = window_spec(3, &options(config));
    assert_eq!(spec.position, Some((12.0, 34.0)));
    assert!(spec.relative_to_host);
    assert!(spec.flags & WINDOW_DECORATED == 0);
    assert!(spec.flags & WINDOW_TRANSPARENT != 0);
    assert!(spec.flags & WINDOW_ALWAYS_ON_TOP != 0);
    assert!(spec.flags & WINDOW_TAKES_FOCUS == 0);
}

#[test]
fn only_windows_and_overlays_are_transparent_when_asked() {
    assert!(!SurfaceKind::Primary.transparent());
    let overlay: Rc<dyn WindowRootDescriptor> = Rc::new(HostOverlayRoot::new("editor"));
    let kind = SurfaceKind::Overlay(overlay);
    assert!(kind.transparent());
    kind.set_content_size(Size::new(40.0, 20.0));
    if let SurfaceKind::Overlay(descriptor) = &kind {
        assert_eq!(descriptor.layout_size(), Size::new(40.0, 20.0));
    }
}
