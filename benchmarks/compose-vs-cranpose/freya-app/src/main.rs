use std::{cell::RefCell, rc::Rc};

use freya::{engine::prelude as skia, prelude::*};
use perf_data::font_bytes;

#[path = "../../wall-canvas/grid.rs"]
mod grid;
#[path = "../../wall-canvas/skia_wall.rs"]
mod skia_wall;

fn face(name: &str) -> skia::Typeface {
    let data = skia::Data::new_copy(&font_bytes(name).unwrap());
    skia::FontMgr::new().new_from_data(data, None).unwrap()
}

fn wall() -> impl IntoElement {
    use perf_data::{FrameIndex, Launch, WALL_CLOCK_HEIGHT, WALL_CLOCK_TEXT, wall_clock};

    let tiles = use_hook(|| {
        Rc::new(RefCell::new(skia_wall::Wall::new(
            face("Roboto-Regular.ttf"),
            face("Roboto-Bold.ttf"),
        )))
    });
    let mut frame = use_state(|| 0u32);
    use_hook(|| {
        let ticker = RenderingTicker::get();
        let window = Platform::get().current_window();
        let mut index = FrameIndex::new(Launch::from_env().freeze);
        spawn(async move {
            loop {
                window.request_redraw();
                ticker.tick().await;
                let more = index.advance();
                frame.set(index.get());
                if !more {
                    break;
                }
            }
        });
    });
    let k = *frame.read();
    let [r, g, b] = grid::BAR;
    rect()
        .expanded()
        .background(Color::from_rgb(15, 18, 32))
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(grid::HEADER))
                .background(Color::from_rgb(r, g, b))
                .main_align(Alignment::Center)
                .padding(Gaps::new(0., 16., 0., 16.))
                .child(
                    label()
                        .text("Still wall")
                        .font_size(20.)
                        .font_weight(FontWeight::BOLD)
                        .color(Color::WHITE),
                ),
        )
        .child(
            rect()
                .width(Size::fill())
                .height(Size::px(WALL_CLOCK_HEIGHT))
                .main_align(Alignment::Center)
                .padding(Gaps::new(0., 8., 0., 8.))
                .child(
                    label()
                        .text(wall_clock(k))
                        .font_size(WALL_CLOCK_TEXT)
                        .color(Color::WHITE),
                ),
        )
        .child(
            rect()
                .expanded()
                .background(RenderCallback::new(move |ctx| {
                    if let RenderTarget::Background { canvas, .. } = &ctx.target {
                        tiles.borrow_mut().draw(
                            canvas,
                            ctx.size.width,
                            ctx.size.height,
                            ctx.scale_factor as f32,
                            k,
                        );
                    }
                })),
        )
}

mod gauntlet;

fn main() {
    perf_data::log_to_stdout();
    let load = perf_data::Launch::from_env();
    let window = match load.scenario {
        perf_data::Scenario::Gauntlet => WindowConfig::new(gauntlet::app).with_title("Gauntlet"),
        perf_data::Scenario::Wall => WindowConfig::new(wall).with_title("Still wall"),
        perf_data::Scenario::Patchwork => panic!("unsupported screen"),
    };
    let (width, height) = perf_data::DESKTOP_WINDOW;
    let mut config = LaunchConfig::new();
    for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
        config = config.with_font("Roboto", Bytes::from(font_bytes(file).unwrap()));
    }
    launch(
        config.with_default_font("Roboto").with_window(
            window
                .with_size(width.into(), height.into())
                .with_resizable(false),
        ),
    );
}
