use std::time::{Duration, Instant};

use cranpose_ui::ImageBitmap;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{EventLoop, run_on_demand::EventLoopExtRunOnDemand},
    platform::x11::EventLoopBuilderExtX11,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::WindowAttributes,
};
use x11rb::{
    connection::Connection,
    protocol::{xfixes::ConnectionExt as _, xproto::ConnectionExt as _},
};

use super::*;

const RED: u32 = 0xffff_0000;
const BLUE: u32 = 0xff00_00ff;

fn solid(argb: u32) -> PointerIcon {
    let rgba = [(argb >> 16) as u8, (argb >> 8) as u8, argb as u8, 0xff];
    let image = ImageBitmap::from_rgba8(8, 8, rgba.repeat(64)).expect("cursor image");
    PointerIcon::custom(image, 0, 0).expect("cursor")
}

fn x11_id(window: &dyn Window) -> u32 {
    match window.window_handle().expect("window handle").as_raw() {
        RawWindowHandle::Xlib(handle) => handle.window as u32,
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        _ => panic!("not an X11 window"),
    }
}

// The first pixel of the cursor the X server shows, once it is `wanted` or the
// wait runs out.
fn shown_cursor(connection: &impl Connection, wanted: u32) -> Option<u32> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let shown = connection
            .xfixes_get_cursor_image()
            .ok()?
            .reply()
            .ok()?
            .cursor_image
            .first()
            .copied();
        if shown == Some(wanted) || Instant::now() > deadline {
            return shown;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "a test waits for another X client's request to land"
        )]
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[derive(Default)]
struct HoverBack {
    window: Option<Arc<dyn Window>>,
    shown: Option<Option<u32>>,
}

impl ApplicationHandler for HoverBack {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let attributes =
            WindowAttributes::default().with_surface_size(winit::dpi::PhysicalSize::new(64, 64));
        self.window = Some(Arc::from(
            event_loop.create_window(attributes).expect("window"),
        ));
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let (WindowEvent::RedrawRequested, Some(window), None) = (event, &self.window, self.shown)
        else {
            return;
        };
        let (probe, _) = x11rb::connect(None).expect("X11 display");
        probe
            .xfixes_query_version(5, 0)
            .expect("XFixes")
            .reply()
            .expect("XFixes");
        probe
            .warp_pointer(x11rb::NONE, x11_id(window.as_ref()), 0, 0, 0, 0, 4, 4)
            .expect("warp");
        probe.flush().expect("flush");

        let mut cursors = DesktopCursors::new(CustomCursorSize::AsDrawn);
        cursors.apply(event_loop, window, &solid(RED));
        cursors.apply(event_loop, window, &solid(BLUE));
        cursors.apply(event_loop, window, &solid(RED));
        self.shown = Some(shown_cursor(&probe, RED));
        event_loop.exit();
    }
}

// Runs under `just x11-cursor`, which provides a disposable X11 display.
#[test]
#[ignore = "requires a disposable X11 display"]
fn a_custom_cursor_shows_again_after_the_window_showed_another() {
    let mut event_loop = EventLoop::builder()
        .with_x11()
        .with_any_thread(true)
        .build()
        .expect("X11 event loop");
    let mut app = HoverBack::default();
    event_loop.run_app_on_demand(&mut app).expect("event loop");

    assert_eq!(
        app.shown,
        Some(Some(RED)),
        "the window shows the red cursor again"
    );
}
