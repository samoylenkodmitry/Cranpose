use vizia::vg::{self as sk, textlayout as para};

use crate::scene::{self, Box2, Face, Font, ImageId, Pictures, Rgb};

fn bounds(b: Box2) -> sk::Rect {
    sk::Rect::from_xywh(b.x, b.y, b.w, b.h)
}
fn color([r, g, b]: Rgb, a: u8) -> sk::Color {
    sk::Color::from_argb(a, r, g, b)
}
fn paint(rgb: Rgb, alpha: u8) -> sk::Paint {
    let mut p = sk::Paint::default();
    p.set_anti_alias(true).set_color(color(rgb, alpha));
    p
}

pub(super) struct Graphics {
    faces: [sk::Typeface; 3],
    fonts: para::FontCollection,
    images: Pictures<sk::Image>,
    layers: [Option<(Box2, sk::Picture)>; 2],
}
impl Graphics {
    pub fn new(faces: [sk::Typeface; 3], scenario: perf_data::Scenario) -> Self {
        let mut provider = para::TypefaceFontProvider::new();
        for (i, f) in faces.iter().enumerate() {
            provider.register_typeface(
                f.clone(),
                if i == 2 { "DejaVu Sans Mono" } else { "Roboto" },
            );
        }
        let mut fonts = para::FontCollection::new();
        fonts.set_asset_font_manager(sk::FontMgr::from(provider));
        let images = Pictures::new(scenario, |b| {
            let info = sk::ImageInfo::new(
                (b.w as i32, b.h as i32),
                sk::ColorType::RGBA8888,
                sk::AlphaType::Unpremul,
                None,
            );
            sk::images::raster_from_data(&info, sk::Data::new_copy(&b.rgba), b.w as usize * 4)
                .expect("benchmark bitmap")
        });
        Self {
            faces,
            fonts,
            images,
            layers: Default::default(),
        }
    }
    pub fn painter<'a>(&'a mut self, canvas: &'a sk::Canvas) -> Painter<'a> {
        Painter {
            canvas,
            gfx: self,
            recording: None,
        }
    }
}

pub(super) struct Painter<'a> {
    canvas: &'a sk::Canvas,
    gfx: &'a mut Graphics,
    recording: Option<sk::PictureRecorder>,
}
impl Painter<'_> {
    fn canvas(&mut self) -> &sk::Canvas {
        match &mut self.recording {
            Some(rec) => rec.recording_canvas().unwrap(),
            None => self.canvas,
        }
    }
    fn font(&self, font: Font) -> sk::Font {
        let i = match font.face {
            Face::Regular => 0,
            Face::Bold => 1,
            Face::Mono => 2,
        };
        let mut f = sk::Font::new(self.gfx.faces[i].clone(), font.size);
        f.set_subpixel(true);
        f
    }
    fn paragraph(&self, text: &str, font: Font, width: f32) -> para::Paragraph {
        let mut style = para::TextStyle::new();
        style
            .set_font_size(font.size)
            .set_color(color(font.color, 255))
            .set_font_families(&[if font.face == Face::Mono {
                "DejaVu Sans Mono"
            } else {
                "Roboto"
            }])
            .set_font_style(if font.face == Face::Bold {
                sk::FontStyle::bold()
            } else {
                sk::FontStyle::normal()
            });
        let mut options = para::ParagraphStyle::new();
        options.set_text_style(&style);
        let mut builder = para::ParagraphBuilder::new(&options, self.gfx.fonts.clone());
        builder.add_text(text);
        let mut paragraph = builder.build();
        paragraph.layout(width.max(1.));
        paragraph
    }
}
impl scene::Painter for Painter<'_> {
    fn rect(&mut self, b: Box2, c: Rgb, a: u8, r: f32) {
        self.canvas().draw_round_rect(bounds(b), r, r, &paint(c, a));
    }
    fn image(&mut self, b: Box2, id: ImageId, r: f32) {
        let image = self.gfx.images.get(id).clone();
        let c = self.canvas();
        c.save();
        if r > 0. {
            c.clip_rrect(sk::RRect::new_rect_xy(bounds(b), r, r), None, true);
        }
        c.draw_image_rect(&image, None, bounds(b), &paint([255; 3], 255));
        c.restore();
    }
    fn size(&mut self, text: &str, font: Font, width: Option<f32>) -> (f32, f32) {
        if let Some(w) = width {
            let p = self.paragraph(text, font, w);
            (p.longest_line(), p.height())
        } else {
            let f = self.font(font);
            let (_, m) = f.metrics();
            (f.measure_str(text, None).0, m.descent - m.ascent)
        }
    }
    fn text(&mut self, b: Box2, text: &str, font: Font, wrap: bool) {
        if b.w <= 0. || b.h <= 0. {
            return;
        }
        if wrap {
            let p = self.paragraph(text, font, b.w);
            let c = self.canvas();
            c.save();
            c.clip_rect(bounds(b), None, false);
            p.paint(c, (b.x, b.y));
            c.restore();
        } else {
            let f = self.font(font);
            let (_, m) = f.metrics();
            let c = self.canvas();
            c.save();
            c.clip_rect(bounds(b), None, false);
            c.draw_str(text, (b.x, b.y - m.ascent), &f, &paint(font.color, 255));
            c.restore();
        }
    }
    fn path(&mut self, points: &[(f32, f32)], c: Rgb, stroke: f32, fill_to: Option<f32>) {
        let mut path = sk::PathBuilder::new();
        let &[first, ref rest @ ..] = points else {
            return;
        };
        path.move_to(first);
        for pt in rest {
            path.line_to(*pt);
        }
        let line = path.snapshot();
        if let Some(bottom) = fill_to {
            let mut fill = path;
            fill.line_to((points.last().unwrap().0, bottom))
                .line_to((first.0, bottom))
                .close();
            let mut brush = paint(c, 64);
            let top = points.iter().map(|pt| pt.1).fold(bottom, f32::min);
            let shader = sk::gradient_shader::linear(
                ((0., top), (0., bottom)),
                &[color(c, 64), color(c, 0)][..],
                None,
                sk::TileMode::Clamp,
                None,
                None,
            );
            brush.set_shader(shader);
            self.canvas().draw_path(&fill.detach(), &brush);
        }
        let mut brush = paint(c, 255);
        brush
            .set_style(sk::PaintStyle::Stroke)
            .set_stroke_width(stroke);
        self.canvas().draw_path(&line, &brush);
    }
    fn clipped(&mut self, b: Box2, draw: impl FnOnce(&mut Self)) {
        self.canvas().save();
        self.canvas().clip_rect(bounds(b), None, false);
        draw(self);
        self.canvas().restore();
    }
    fn rotated(&mut self, center: (f32, f32), degrees: f32, draw: impl FnOnce(&mut Self)) {
        self.canvas().save();
        self.canvas().rotate(degrees, Some(center.into()));
        draw(self);
        self.canvas().restore();
    }
    fn cached(&mut self, i: usize, b: Box2, draw: impl FnOnce(&mut Self)) {
        if self.gfx.layers[i]
            .as_ref()
            .is_none_or(|(previous, _)| *previous != b)
        {
            let mut rec = sk::PictureRecorder::new();
            rec.begin_recording(bounds(b), false);
            self.recording = Some(rec);
            draw(self);
            let picture = self
                .recording
                .take()
                .unwrap()
                .finish_recording_as_picture(None)
                .unwrap();
            self.gfx.layers[i] = Some((b, picture));
        }
        let picture = &self.gfx.layers[i].as_ref().unwrap().1;
        self.canvas.draw_picture(picture, None, None);
    }
}
