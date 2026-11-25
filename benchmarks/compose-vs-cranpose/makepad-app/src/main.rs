use makepad_widgets::*;
use perf_data::*;

#[path = "../../wall-canvas/grid.rs"]
mod grid;
use grid::{BAR, Grid, HEADER};

live_design! {
    use link::widgets::*;
    use link::shaders::*;
    TileShape = {{TileShape}} {
        fn pixel(self) -> vec4 {
            if self.radius == 0.0 && self.stroke == 0.0 { return self.color; }
            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
            let inset = self.stroke * 0.5;
            // Sdf2d.box uses twice its radius parameter as the arc radius.
            let corner = max(self.radius - inset, 0.0) * 0.5;
            sdf.box(inset, inset, self.rect_size.x - 2.0*inset, self.rect_size.y - 2.0*inset, corner);
            if self.stroke > 0.0 { return sdf.stroke(self.color, self.stroke); }
            return sdf.fill(self.color);
        }
    }
    Wall = {{Wall}} {
        width: Fill, height: Fill,
        shape: <TileShape> {}
        regular: {text_style: {font_family: {latin = font("crate://self/fonts/Roboto-Regular.ttf", 0.0, 0.0)}}}
        bold: {text_style: {font_family: {latin = font("crate://self/fonts/Roboto-Bold.ttf", 0.0, 0.0)}}}
    }
    App = {{App}} {
        ui: <Root> { main_window = <Window> {
            window: { inner_size: vec2(1280, 848), title:"Still wall" }
            caption_bar = { visible: true, height: 28, caption_label = { label = { text: "Still wall" } } }
            body = <View> { <Wall> {} }
        } }
    }
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
struct TileShape {
    #[deref]
    base: DrawQuad,
    #[live]
    color: Vec4,
    #[live]
    radius: f32,
    #[live]
    stroke: f32,
}

impl TileShape {
    fn rect(&mut self, cx: &mut Cx2d, pos: DVec2, w: f32, h: f32, rgb: [u8; 3], radius: f32) {
        let [r, g, b] = rgb.map(|v| f32::from(v) / 255.0);
        self.color = vec4(r, g, b, 1.);
        self.radius = radius.min(w / 2.).min(h / 2.);
        self.draw_abs(
            cx,
            Rect {
                pos,
                size: dvec2(w.into(), h.into()),
            },
        );
    }
}

#[derive(Live, LiveHook, Widget)]
struct Wall {
    #[walk]
    walk: Walk,
    #[area]
    area: Area,
    #[redraw]
    #[live]
    shape: TileShape,
    #[live]
    regular: DrawText,
    #[live]
    bold: DrawText,
    #[live]
    tiles: DrawList2d,
    #[rust]
    size: Option<Rect>,
    #[rust]
    animation: Option<(WallTier, FrameIndex)>,
    #[rust]
    tick: NextFrame,
}

impl Wall {
    fn tiles(&mut self, cx: &mut Cx2d, origin: DVec2, grid: Grid) {
        let pos = |index| {
            let (x, y) = grid.origin(index);
            origin + dvec2(x.into(), y.into())
        };
        let (w, h) = (grid.tier.tile_width, grid.tier.tile_height);
        let dot = wall_dot_size(h);
        let pad = f64::from(wall_padding(h));
        let (label, value) = wall_text_size(h);
        self.bold.text_style.font_size = label * 0.75;
        self.regular.text_style.font_size = value * 0.75;
        for index in 0..grid.tiles {
            self.shape
                .rect(cx, pos(index), w, h, wall_fill_rgb(index), WALL_CORNER);
        }
        for index in 0..grid.tiles {
            let line = pos(index) + dvec2(pad, pad + f64::from(label * 1.17));
            self.shape.rect(
                cx,
                line + dvec2(0., f64::from(value * 0.6 - dot * 0.5)),
                dot,
                dot,
                wall_dot_rgb(index),
                dot / 2.,
            );
        }
        for index in 0..grid.tiles {
            let at = pos(index) + dvec2(pad, pad);
            self.bold.draw_abs(cx, at, &wall_label(index));
            self.regular.draw_abs(
                cx,
                at + dvec2(f64::from(dot * 1.5), f64::from(label * 1.17)),
                &wall_value(index),
            );
        }
    }
}

impl Widget for Wall {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, _scope: &mut Scope) {
        if self.tick.is_event(event).is_some() {
            self.area.redraw(cx);
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let rect = cx.walk_turtle_with_area(&mut self.area, walk);
        let (tier, frame) = self.animation.get_or_insert_with(|| {
            let load = Launch::from_env();
            assert_eq!(load.scenario, Scenario::Wall);
            (wall_tier(load.tier), FrameIndex::new(load.freeze))
        });
        let tier = *tier;
        let more = frame.advance();
        let k = frame.get();
        let (w, h) = (rect.size.x as f32, rect.size.y as f32);
        let offset = HEADER + WALL_CLOCK_HEIGHT;
        let origin = rect.pos + dvec2(0., offset.into());
        let grid = Grid::new(tier, w, h - offset);
        self.shape.stroke = 0.;
        self.shape.rect(cx, rect.pos, w, h, WALL_BACKGROUND_RGB, 0.);
        self.shape.rect(cx, rect.pos, w, HEADER, BAR, 0.);
        self.bold.text_style.font_size = 15.;
        self.bold
            .draw_abs(cx, rect.pos + dvec2(16., 16.), "Still wall");
        self.regular.text_style.font_size = WALL_CLOCK_TEXT * 0.75;
        self.regular
            .draw_abs(cx, rect.pos + dvec2(8., 59.), &wall_clock(k));
        if self
            .tiles
            .begin_maybe(cx, self.size != Some(rect))
            .is_redrawing()
        {
            self.tiles(cx, origin, grid);
            self.tiles.end(cx);
            self.size = Some(rect);
        }
        let (x, y) = grid.origin(wall_cursor(k, grid.tiles));
        self.shape.stroke = WALL_CURSOR_WIDTH;
        self.shape.rect(
            cx,
            origin + dvec2(x.into(), y.into()),
            grid.tier.tile_width,
            grid.tier.tile_height,
            WALL_INK_RGB,
            WALL_CORNER,
        );
        if more {
            self.tick = cx.new_next_frame();
        }
        DrawStep::done()
    }
}

fn main() {
    perf_data::log_to_stdout();
    app_main();
}
app_main!(App);
#[derive(Live, LiveHook)]
struct App {
    #[live]
    ui: WidgetRef,
}

impl LiveRegister for App {
    fn live_register(cx: &mut Cx) {
        makepad_widgets::live_design(cx);
    }
}

impl AppMain for App {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}
