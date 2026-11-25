use std::cell::{Cell, RefCell};

use perf_data::{DESKTOP_WINDOW, FrameIndex, Launch, Scenario, font_bytes, wall_clock};
use vizia::prelude::*;
mod painter;
mod skia {
    pub use vizia::vg::{surfaces::raster_n32_premul, *};
}
#[path = "../../wall-canvas/grid.rs"]
mod grid;
#[path = "../../native-canvas/mod.rs"]
mod scene;
#[path = "../../wall-canvas/skia_wall.rs"]
mod skia_wall;

enum Screen {
    Wall(skia_wall::Wall),
    Other(scene::Workload, painter::Graphics),
}
struct BenchView {
    screen: RefCell<Screen>,
    frame: Cell<FrameIndex>,
    clock: Option<Signal<String>>,
}
impl View for BenchView {
    fn draw(&self, cx: &mut DrawContext, canvas: &Canvas) {
        let bounds = cx.bounds();
        let scale = cx.scale_factor();
        let mut frame = self.frame.get();
        let before = frame.get();
        let more = frame.advance();
        self.frame.set(frame);
        let changed = before != frame.get();
        canvas.save();
        canvas.translate((bounds.x, bounds.y)).scale((scale, scale));
        let (w, h) = (bounds.w / scale, bounds.h / scale);
        match &mut *self.screen.borrow_mut() {
            Screen::Wall(wall) => wall.draw(canvas, w, h, scale, frame.get()),
            Screen::Other(scene, gfx) => scene.draw(
                &mut gfx.painter(canvas),
                scene::Box2::new(0., 0., w, h),
                frame.get(),
                changed,
            ),
        }
        canvas.restore();
        if changed && let Some(clock) = self.clock {
            clock.set(wall_clock(frame.get()));
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
    let load = Launch::from_env();
    let title = load.scenario.title();
    let (width, height) = DESKTOP_WINDOW;
    Application::new(move |cx| {
        let normal = font_bytes("Roboto-Regular.ttf").unwrap();
        let bold = font_bytes("Roboto-Bold.ttf").unwrap();
        let (n, b) = (face(&normal), face(&bold));
        cx.add_font_mem(normal);
        cx.add_font_mem(bold);
        let screen = if let Some(workload) = scene::Workload::new(load) {
            let mono = face(&font_bytes("DejaVuSansMono.ttf").unwrap());
            Screen::Other(
                workload,
                painter::Graphics::new([n, b, mono], load.scenario),
            )
        } else {
            Screen::Wall(skia_wall::Wall::new(n, b))
        };
        cx.add_stylesheet(include_str!("wall.css")).unwrap();
        let clock = (load.scenario != Scenario::Gauntlet).then(|| Signal::new(wall_clock(0)));
        let background = if load.scenario == Scenario::Gauntlet {
            perf_data::GAUNTLET_BACKGROUND_RGB
        } else {
            perf_data::WALL_BACKGROUND_RGB
        };
        VStack::new(cx, |cx| {
            let [r, g, b] = grid::BAR;
            Label::new(cx, title)
                .class("header")
                .height(Pixels(grid::HEADER))
                .background_color(Color::rgb(r, g, b));
            if let Some(clock) = clock {
                Label::new(cx, clock).class("clock");
            }
            BenchView {
                screen: RefCell::new(screen),
                frame: Cell::new(FrameIndex::new(load.freeze)),
                clock,
            }
            .build(cx, |_| {});
        })
        .background_color(Color::rgb(background[0], background[1], background[2]));
    })
    .title(title)
    .inner_size((width, height))
    .resizable(false)
    .run()
}
