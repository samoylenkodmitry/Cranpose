use std::cell::{Cell, RefCell};

use perf_data::{DESKTOP_WINDOW, FrameIndex, Launch, font_bytes, wall_clock};
use vizia::prelude::*;

mod skia {
    pub use vizia::vg::{surfaces::raster_n32_premul, *};
}

#[path = "../../wall-canvas/grid.rs"]
mod grid;
#[path = "../../wall-canvas/skia_wall.rs"]
mod skia_wall;

struct WallView {
    tiles: RefCell<skia_wall::Wall>,
    frame: Cell<FrameIndex>,
    clock: Signal<String>,
}

impl View for WallView {
    fn draw(&self, cx: &mut DrawContext, canvas: &Canvas) {
        let bounds = cx.bounds();
        let scale = cx.scale_factor();
        let mut frame = self.frame.get();
        let previous = frame.get();
        let more = frame.advance();
        self.frame.set(frame);
        canvas.save();
        canvas.translate((bounds.x, bounds.y)).scale((scale, scale));
        self.tiles.borrow_mut().draw(
            canvas,
            bounds.w / scale,
            bounds.h / scale,
            scale,
            frame.get(),
        );
        canvas.restore();
        if previous != frame.get() {
            self.clock.set(wall_clock(frame.get()));
        }
        if more {
            cx.needs_redraw();
        }
    }
}

fn face(bytes: &[u8]) -> skia::Typeface {
    skia::FontMgr::new().new_from_data(bytes, None).unwrap()
}

fn main() -> Result<(), ApplicationError> {
    perf_data::log_to_stdout();
    let (width, height) = DESKTOP_WINDOW;
    Application::new(|cx| {
        let normal = font_bytes("Roboto-Regular.ttf").unwrap();
        let bold = font_bytes("Roboto-Bold.ttf").unwrap();
        let tiles = skia_wall::Wall::new(face(&normal), face(&bold));
        cx.add_font_mem(normal);
        cx.add_font_mem(bold);
        cx.add_stylesheet(include_str!("wall.css")).unwrap();
        let clock = Signal::new(wall_clock(0));
        VStack::new(cx, |cx| {
            let [r, g, b] = grid::BAR;
            Label::new(cx, "Still wall")
                .class("header")
                .height(Pixels(grid::HEADER))
                .background_color(Color::rgb(r, g, b));
            Label::new(cx, clock).class("clock");
            WallView {
                tiles: RefCell::new(tiles),
                frame: Cell::new(FrameIndex::new(Launch::from_env().freeze)),
                clock,
            }
            .build(cx, |_| {});
        });
    })
    .title("Still wall")
    .inner_size((width as u32, height as u32))
    .resizable(false)
    .run()
}
