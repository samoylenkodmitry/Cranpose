//! Custom cursors that keep the size they were drawn at on macOS.
//!
//! macOS enlarges every cursor by its accessibility pointer size, an app's own
//! images included. A cursor asked for as drawn keeps every pixel of its image
//! but is described to AppKit as that many points divided by the pointer size,
//! which the system then enlarges back to the size it was drawn at.
//!
//! winit accepts only cursors it built itself, one point to a pixel, so a
//! cursor sized this way is shown from here. AppKit re-applies a window's
//! cursor rectangles on its own schedule -- entering the window, activating it,
//! resizing it -- and those rectangles carry winit's cursor, so they are held
//! off while such a cursor is up and handed back as soon as it is not.
#![allow(unsafe_code)]

use std::ffi::c_uchar;

use objc2::{AllocAnyThread, rc::Retained};
use objc2_app_kit::{NSBitmapImageRep, NSCursor, NSDeviceRGBColorSpace, NSImage, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSSize, NSUserDefaults, ns_string};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// How much macOS enlarges every cursor: the accessibility pointer size, from
/// 1 to 4, and 1 when it was never set.
pub(crate) fn pointer_scale() -> f64 {
    let defaults = NSUserDefaults::initWithSuiteName(
        NSUserDefaults::alloc(),
        Some(ns_string!("com.apple.universalaccess")),
    );
    let scale = defaults
        .map(|defaults| defaults.doubleForKey(ns_string!("mouseDriverCursorSize")))
        .unwrap_or(1.0);
    crate::cursor_scale::usable_scale(scale)
}

/// A cursor built to appear at the size its image was drawn at.
pub(crate) struct AsDrawnCursor(Retained<NSCursor>);

/// The RGBA `pixels`, `width` by `height`, as a cursor whose image and hotspot
/// are `scale` times smaller in points than in pixels.
pub(crate) fn as_drawn(
    pixels: &[u8],
    width: u32,
    height: u32,
    hotspot: (u32, u32),
    scale: f64,
) -> Option<AsDrawnCursor> {
    let row = width as usize * 4;
    if width == 0 || height == 0 || pixels.len() != row * height as usize {
        return None;
    }
    // SAFETY: a null plane list makes AppKit allocate the bitmap's own storage,
    // sized by the dimensions given, which is then filled below.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut::<*mut c_uchar>(),
            width as isize,
            height as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            row as isize,
            32,
        )
    }?;
    // SAFETY: the storage AppKit allocated above is `bytesPerRow * pixelsHigh`
    // bytes, exactly `pixels.len()`, and nothing else refers to it yet.
    let storage = unsafe { std::slice::from_raw_parts_mut(bitmap.bitmapData(), pixels.len()) };
    storage.copy_from_slice(pixels);

    let size = NSSize::new(f64::from(width) / scale, f64::from(height) / scale);
    bitmap.setSize(size);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    image.addRepresentation(&bitmap);
    let hotspot = NSPoint::new(f64::from(hotspot.0) / scale, f64::from(hotspot.1) / scale);
    Some(AsDrawnCursor(NSCursor::initWithImage_hotSpot(
        NSCursor::alloc(),
        &image,
        hotspot,
    )))
}

fn ns_window(window: &dyn Window) -> Option<Retained<NSWindow>> {
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit hands out the content view of a window that is still open,
    // and the event loop this is called from runs on the main thread AppKit
    // views belong to.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

/// Shows `cursor` over `window`, holding the window's cursor rectangles off
/// so AppKit does not put winit's cursor back over it.
pub(crate) fn hold(window: &dyn Window, cursor: &AsDrawnCursor) {
    if let Some(window) = ns_window(window) {
        window.disableCursorRects();
    }
    cursor.0.set();
}

/// Shows `cursor` again. Entering a window, the window server puts its own
/// arrow up over a window whose rectangles are held off, after the cursor was
/// set, and without AppKit's `currentCursor` noticing.
pub(crate) fn keep(cursor: &AsDrawnCursor) {
    cursor.0.set();
}

/// Hands `window`'s cursor back to its cursor rectangles, and so to winit.
pub(crate) fn release(window: &dyn Window) {
    if let Some(window) = ns_window(window) {
        window.enableCursorRects();
    }
}
