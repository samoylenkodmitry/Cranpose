// The pictures on the stamps: each a small drawing of the demo tab its stamp
// opens, drawn on the stamp's own size.

use cranpose_ui::{
    composable, Brush, Canvas, Color, CornerRadii, DrawScope, DrawTextStyle, Modifier, Point, Rect,
    Stroke, StrokeCap,
};

use super::lettering::{DISPLAY_FAMILY, STRONG_FAMILY};
use crate::app::DemoTab;

// The tabs the stamps open, in the sheet's order.
pub(super) const STAMP_TABS: [DemoTab; 15] = [
    DemoTab::Counter,
    DemoTab::Animations,
    DemoTab::Mineswapper2,
    DemoTab::HackerNews,
    DemoTab::Text,
    DemoTab::Shaders,
    DemoTab::Controls,
    DemoTab::GlassTiles,
    DemoTab::MarkdownViewer,
    DemoTab::FloatingWindows,
    DemoTab::Room,
    DemoTab::Backrooms,
    DemoTab::Tv,
    DemoTab::Music,
    DemoTab::Wear,
];

// The picture of `tab`'s stamp.
#[composable(no_skip)]
pub(super) fn StampArt(tab: DemoTab, modifier: Modifier) {
    Canvas(modifier, move |scope| {
        let draw: fn(&mut dyn DrawScope, Frame) = match tab {
            DemoTab::Counter => counter,
            DemoTab::Animations => bouncing_ball,
            DemoTab::Mineswapper2 => mines,
            DemoTab::HackerNews => news,
            DemoTab::Text => letters,
            DemoTab::Shaders => plasma,
            DemoTab::Controls => controls,
            DemoTab::GlassTiles => glass_tiles,
            DemoTab::MarkdownViewer => markdown,
            DemoTab::FloatingWindows => windows,
            DemoTab::Room => room,
            DemoTab::Backrooms => backrooms,
            DemoTab::Tv => tv,
            DemoTab::Music => piano_roll,
            _ => watch,
        };
        let size = scope.size();
        draw(
            scope,
            Frame {
                w: size.width,
                h: size.height,
            },
        );
    });
}

// The picture's size, with points and rects given in shares of it.
#[derive(Clone, Copy)]
struct Frame {
    w: f32,
    h: f32,
}

impl Frame {
    fn at(self, x: f32, y: f32) -> Point {
        Point::new(x * self.w, y * self.h)
    }

    fn rect(self, x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x: x * self.w,
            y: y * self.h,
            width: width * self.w,
            height: height * self.h,
        }
    }
}

fn solid(color: Color) -> Brush {
    Brush::solid(color)
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    )
}

fn type_style(family: &str, size: f32) -> DrawTextStyle {
    DrawTextStyle {
        font_family: Some(family.to_owned()),
        font_size: size,
        ..DrawTextStyle::default()
    }
}

fn counter(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(Brush::vertical_gradient(
        vec![rgb(214, 232, 255), rgb(170, 200, 246)],
        0.0,
        b.h,
    ));
    scope.draw_text_from(
        b.at(0.34, 0.14),
        solid(rgb(20, 36, 90)),
        "42",
        &type_style(DISPLAY_FAMILY, b.h * 0.5),
    );
    for (x, sign) in [(0.17, "-"), (0.83, "+")] {
        scope.draw_circle(solid(rgb(40, 80, 200)), b.at(x, 0.5), b.h * 0.16);
        scope.draw_text_from(
            b.at(x - 0.035, 0.34),
            solid(Color::WHITE),
            sign,
            &type_style(STRONG_FAMILY, b.h * 0.26),
        );
    }
}

fn bouncing_ball(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(24, 22, 54)));
    scope.draw_line(
        solid(rgb(90, 86, 150)),
        b.at(0.05, 0.86),
        b.at(0.95, 0.86),
        Stroke::new(b.h * 0.02),
    );
    for step in 0..9 {
        let t = step as f32 / 8.0;
        let bounce = (t * 2.0 * std::f32::consts::PI).sin().abs();
        let alpha = 0.15 + 0.85 * t;
        let ball = Color(1.0, 0.45 + 0.3 * t, 0.2, alpha);
        scope.draw_circle(
            solid(ball),
            b.at(0.1 + 0.8 * t, 0.76 - 0.55 * bounce),
            b.h * 0.07,
        );
    }
}

fn mines(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(190, 192, 200)));
    let (columns, rows) = (8, 5);
    let cell = (b.w / columns as f32).min(b.h / rows as f32);
    let numbers = [
        (1, 1, "1", rgb(30, 60, 220)),
        (2, 1, "2", rgb(20, 130, 40)),
        (3, 2, "3", rgb(210, 30, 30)),
        (1, 3, "1", rgb(30, 60, 220)),
    ];
    for row in 0..rows {
        for column in 0..columns {
            let rect = Rect {
                x: column as f32 * cell + 1.0,
                y: row as f32 * cell + 1.0,
                width: cell - 2.0,
                height: cell - 2.0,
            };
            let open = column < 4 && row > 0 && row < 4;
            scope.draw_rect_at(
                rect,
                solid(if open {
                    rgb(222, 222, 226)
                } else {
                    rgb(150, 154, 168)
                }),
            );
            if let Some((_, _, digit, color)) = numbers
                .iter()
                .find(|(x, y, _, _)| *x == column && *y == row)
            {
                scope.draw_text_from(
                    Point::new(rect.x + cell * 0.28, rect.y),
                    solid(*color),
                    digit,
                    &type_style(STRONG_FAMILY, cell * 0.8),
                );
            }
        }
    }
    let flag = Point::new(5.5 * cell, 2.5 * cell);
    scope.draw_line(
        solid(Color::BLACK),
        Point::new(flag.x, flag.y - cell * 0.35),
        Point::new(flag.x, flag.y + cell * 0.35),
        Stroke::new(2.0),
    );
    let mut pennant = cranpose_ui::Path::new();
    pennant.move_to(Point::new(flag.x, flag.y - cell * 0.35));
    pennant.line_to(Point::new(flag.x + cell * 0.35, flag.y - cell * 0.18));
    pennant.line_to(Point::new(flag.x, flag.y));
    pennant.close();
    scope.draw_path(
        &pennant,
        solid(rgb(220, 30, 30)),
        cranpose_ui::DrawStyle::Fill,
    );
}

fn news(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(246, 246, 239)));
    scope.draw_rect_at(b.rect(0.0, 0.0, 1.0, 0.2), solid(rgb(255, 102, 0)));
    scope.draw_rect_at(b.rect(0.03, 0.04, 0.08, 0.12), solid(Color::WHITE));
    scope.draw_text_from(
        b.at(0.045, 0.02),
        solid(rgb(255, 102, 0)),
        "Y",
        &type_style(STRONG_FAMILY, b.h * 0.14),
    );
    for row in 0..5 {
        let y = 0.28 + row as f32 * 0.14;
        let length = 0.55 + 0.3 * ((row * 7 % 5) as f32 / 4.0);
        scope.draw_rect_at(b.rect(0.06, y, length, 0.05), solid(rgb(60, 60, 60)));
        scope.draw_rect_at(
            b.rect(0.06, y + 0.065, length * 0.5, 0.03),
            solid(rgb(150, 150, 140)),
        );
    }
}

fn letters(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(252, 248, 238)));
    scope.draw_text_from(
        b.at(0.08, 0.02),
        solid(rgb(150, 40, 60)),
        "Aa",
        &type_style(DISPLAY_FAMILY, b.h * 0.72),
    );
    scope.draw_text_from(
        b.at(0.62, 0.18),
        solid(rgb(40, 40, 60)),
        "Qg",
        &type_style(STRONG_FAMILY, b.h * 0.3),
    );
    scope.draw_text_from(
        b.at(0.62, 0.56),
        solid(rgb(40, 90, 180)),
        "fi",
        &type_style(DISPLAY_FAMILY, b.h * 0.26),
    );
}

fn plasma(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(Brush::sweep_gradient(
        vec![
            rgb(255, 60, 120),
            rgb(255, 200, 40),
            rgb(40, 220, 160),
            rgb(60, 90, 255),
            rgb(255, 60, 120),
        ],
        b.at(0.5, 0.5),
    ));
    for ring in 0..4 {
        let radius = b.h * (0.12 + 0.13 * ring as f32);
        scope.draw_circle_stroked(
            solid(Color(1.0, 1.0, 1.0, 0.35)),
            b.at(0.5, 0.5),
            radius,
            Stroke::new(b.h * 0.03),
        );
    }
}

fn controls(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(36, 40, 52)));
    let knob = b.at(0.27, 0.5);
    scope.draw_circle(solid(rgb(90, 96, 116)), knob, b.h * 0.3);
    scope.draw_circle(solid(rgb(130, 138, 160)), knob, b.h * 0.24);
    scope.draw_line(
        solid(rgb(255, 170, 60)),
        knob,
        Point::new(knob.x + b.h * 0.15, knob.y - b.h * 0.14),
        Stroke::new(b.h * 0.05).with_cap(StrokeCap::Round),
    );
    scope.draw_round_rect_at(
        b.rect(0.58, 0.18, 0.3, 0.2),
        solid(rgb(80, 200, 120)),
        CornerRadii::uniform(b.h * 0.1),
    );
    scope.draw_circle(solid(Color::WHITE), b.at(0.82, 0.28), b.h * 0.08);
    scope.draw_line(
        solid(rgb(110, 116, 140)),
        b.at(0.58, 0.72),
        b.at(0.9, 0.72),
        Stroke::new(b.h * 0.04).with_cap(StrokeCap::Round),
    );
    scope.draw_circle(solid(rgb(255, 170, 60)), b.at(0.7, 0.72), b.h * 0.07);
}

fn glass_tiles(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(Brush::linear_gradient_range(
        vec![rgb(255, 120, 80), rgb(120, 60, 220), rgb(30, 180, 220)],
        b.at(0.0, 0.0),
        b.at(1.0, 1.0),
    ));
    scope.draw_circle(solid(rgb(255, 220, 80)), b.at(0.7, 0.3), b.h * 0.25);
    for (x, y) in [
        (0.08, 0.14),
        (0.38, 0.14),
        (0.68, 0.14),
        (0.08, 0.56),
        (0.38, 0.56),
        (0.68, 0.56),
    ] {
        scope.draw_round_rect_at(
            b.rect(x, y, 0.25, 0.32),
            solid(Color(1.0, 1.0, 1.0, 0.32)),
            CornerRadii::uniform(b.h * 0.06),
        );
    }
}

fn markdown(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(Color::WHITE));
    scope.draw_text_from(
        b.at(0.06, 0.04),
        solid(rgb(30, 30, 40)),
        "# Notes",
        &type_style(DISPLAY_FAMILY, b.h * 0.2),
    );
    scope.draw_round_rect_at(
        b.rect(0.06, 0.38, 0.88, 0.24),
        solid(rgb(238, 240, 246)),
        CornerRadii::uniform(4.0),
    );
    for (row, length) in [(0.43, 0.5), (0.51, 0.62)] {
        scope.draw_rect_at(b.rect(0.1, row, length, 0.04), solid(rgb(120, 70, 170)));
    }
    for row in 0..2 {
        let y = 0.72 + row as f32 * 0.12;
        scope.draw_circle(solid(rgb(30, 30, 40)), b.at(0.09, y + 0.025), b.h * 0.02);
        scope.draw_rect_at(
            b.rect(0.14, y, 0.5 + 0.2 * row as f32, 0.05),
            solid(rgb(90, 90, 100)),
        );
    }
}

fn windows(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(40, 120, 130)));
    for (x, y, color) in [
        (0.06, 0.1, rgb(250, 240, 220)),
        (0.32, 0.3, rgb(230, 240, 255)),
        (0.56, 0.18, rgb(255, 230, 235)),
    ] {
        let frame = b.rect(x, y, 0.4, 0.55);
        scope.draw_round_rect_at(frame, solid(color), CornerRadii::uniform(4.0));
        scope.draw_round_rect_at(
            Rect {
                height: b.h * 0.1,
                ..frame
            },
            solid(rgb(60, 64, 80)),
            CornerRadii::uniform(4.0),
        );
        for (dot, tint) in [
            (0.0, rgb(255, 95, 86)),
            (1.0, rgb(255, 189, 46)),
            (2.0, rgb(39, 201, 63)),
        ] {
            scope.draw_circle(
                solid(tint),
                Point::new(frame.x + b.h * (0.05 + 0.06 * dot), frame.y + b.h * 0.05),
                b.h * 0.022,
            );
        }
    }
}

fn room(scope: &mut dyn DrawScope, b: Frame) {
    let back = b.rect(0.3, 0.22, 0.4, 0.42);
    scope.draw_rect(solid(rgb(120, 90, 70)));
    let mut floor = cranpose_ui::Path::new();
    floor.move_to(b.at(0.0, 1.0));
    floor.line_to(Point::new(back.x, back.y + back.height));
    floor.line_to(Point::new(back.x + back.width, back.y + back.height));
    floor.line_to(b.at(1.0, 1.0));
    floor.close();
    scope.draw_path(
        &floor,
        solid(rgb(170, 120, 80)),
        cranpose_ui::DrawStyle::Fill,
    );
    scope.draw_rect_at(back, solid(rgb(210, 200, 180)));
    scope.draw_rect_at(
        Rect {
            x: back.x + back.width * 0.55,
            y: back.y + back.height * 0.2,
            width: back.width * 0.3,
            height: back.height * 0.4,
        },
        solid(rgb(150, 200, 240)),
    );
    for edge in [
        b.at(0.0, 0.0),
        b.at(1.0, 0.0),
        b.at(0.0, 1.0),
        b.at(1.0, 1.0),
    ] {
        let corner = Point::new(
            if edge.x > 0.0 {
                back.x + back.width
            } else {
                back.x
            },
            if edge.y > 0.0 {
                back.y + back.height
            } else {
                back.y
            },
        );
        scope.draw_line(solid(rgb(80, 60, 50)), edge, corner, Stroke::new(1.5));
    }
}

fn backrooms(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(196, 178, 96)));
    scope.draw_rect_at(b.rect(0.0, 0.0, 1.0, 0.24), solid(rgb(222, 214, 170)));
    scope.draw_rect_at(b.rect(0.0, 0.78, 1.0, 0.22), solid(rgb(150, 128, 70)));
    scope.draw_rect_at(b.rect(0.4, 0.3, 0.2, 0.44), solid(rgb(120, 104, 54)));
    for x in [0.12, 0.42, 0.72] {
        scope.draw_rect_at(b.rect(x, 0.06, 0.16, 0.06), solid(rgb(255, 255, 236)));
    }
    for x in [0.05, 0.25, 0.75, 0.95] {
        scope.draw_line(
            solid(rgb(170, 152, 80)),
            b.at(x, 0.24),
            b.at(x, 0.78),
            Stroke::new(1.0),
        );
    }
}

fn tv(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(50, 36, 30)));
    let screen = b.rect(0.16, 0.12, 0.68, 0.66);
    scope.draw_round_rect_at(
        b.rect(0.1, 0.06, 0.8, 0.78),
        solid(rgb(30, 30, 34)),
        CornerRadii::uniform(b.h * 0.08),
    );
    let bars = [
        rgb(240, 240, 240),
        rgb(240, 230, 40),
        rgb(40, 230, 230),
        rgb(40, 220, 40),
        rgb(230, 40, 230),
        rgb(230, 40, 40),
        rgb(40, 40, 230),
    ];
    let width = screen.width / bars.len() as f32;
    for (index, color) in bars.iter().enumerate() {
        scope.draw_rect_at(
            Rect {
                x: screen.x + width * index as f32,
                width,
                ..screen
            },
            solid(*color),
        );
    }
    scope.draw_line(
        solid(rgb(30, 30, 34)),
        b.at(0.35, 0.84),
        b.at(0.28, 0.97),
        Stroke::new(3.0),
    );
    scope.draw_line(
        solid(rgb(30, 30, 34)),
        b.at(0.65, 0.84),
        b.at(0.72, 0.97),
        Stroke::new(3.0),
    );
}

fn piano_roll(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(28, 30, 44)));
    let rows = 10;
    let row = b.h / rows as f32;
    for index in 0..rows {
        let black = matches!(index % 7, 1 | 3 | 6);
        scope.draw_rect_at(
            Rect {
                x: 0.0,
                y: index as f32 * row + 0.5,
                width: b.w * 0.14,
                height: row - 1.0,
            },
            solid(if black {
                rgb(20, 20, 24)
            } else {
                rgb(236, 236, 240)
            }),
        );
    }
    let notes = [
        (0.2, 2, 0.18),
        (0.4, 4, 0.12),
        (0.5, 1, 0.22),
        (0.62, 6, 0.1),
        (0.72, 3, 0.2),
        (0.3, 7, 0.16),
        (0.8, 5, 0.12),
    ];
    for (start, pitch, length) in notes {
        let hue = Color(
            0.3 + 0.07 * pitch as f32,
            0.9 - 0.08 * pitch as f32,
            0.95,
            1.0,
        );
        scope.draw_round_rect_at(
            b.rect(start, pitch as f32 / rows as f32 + 0.01, length, 0.08),
            solid(hue),
            CornerRadii::uniform(2.0),
        );
    }
}

fn watch(scope: &mut dyn DrawScope, b: Frame) {
    scope.draw_rect(solid(rgb(60, 70, 90)));
    let centre = b.at(0.5, 0.5);
    scope.draw_round_rect_at(
        b.rect(0.42, 0.0, 0.16, 1.0),
        solid(rgb(30, 30, 36)),
        CornerRadii::uniform(4.0),
    );
    scope.draw_circle(solid(rgb(20, 20, 24)), centre, b.h * 0.44);
    scope.draw_circle(solid(rgb(12, 14, 30)), centre, b.h * 0.38);
    for tick in 0..12 {
        let angle = tick as f32 * std::f32::consts::PI / 6.0;
        let (sin, cos) = angle.sin_cos();
        scope.draw_line(
            solid(rgb(200, 210, 230)),
            Point::new(centre.x + sin * b.h * 0.3, centre.y - cos * b.h * 0.3),
            Point::new(centre.x + sin * b.h * 0.35, centre.y - cos * b.h * 0.35),
            Stroke::new(1.5),
        );
    }
    scope.draw_line(
        solid(Color::WHITE),
        centre,
        Point::new(centre.x + b.h * 0.15, centre.y - b.h * 0.12),
        Stroke::new(3.0).with_cap(StrokeCap::Round),
    );
    scope.draw_line(
        solid(rgb(255, 120, 60)),
        centre,
        Point::new(centre.x - b.h * 0.05, centre.y - b.h * 0.27),
        Stroke::new(2.0).with_cap(StrokeCap::Round),
    );
}
