use makepad_widgets::*;

use crate::{
    TileShape,
    scene::{self, Box2, Face, Font, ImageId, Pictures, Rgb},
};

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawBitmap {
    #[deref]
    base: DrawQuad,
    #[live]
    radius: f32,
}
#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawBand {
    #[deref]
    base: DrawQuad,
    #[live]
    left_y: f32,
    #[live]
    right_y: f32,
    #[live]
    color: Vec4,
}

pub(super) struct Graphics {
    images: Pictures<Texture>,
    static_lists: [(DrawList2d, Option<Box2>); 2],
    groups: Vec<DrawList2d>,
}
impl Graphics {
    pub fn new(cx: &mut Cx, scenario: perf_data::Scenario) -> Self {
        let images = Pictures::new(scenario, |b| {
            let data = b
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from_le_bytes([p[2], p[1], p[0], p[3]]))
                .collect();
            Texture::new_with_format(
                cx,
                TextureFormat::VecBGRAu8_32 {
                    width: b.w as usize,
                    height: b.h as usize,
                    data: Some(data),
                    updated: TextureUpdated::Full,
                },
            )
        });
        Self {
            images,
            static_lists: std::array::from_fn(|_| (DrawList2d::new(cx), None)),
            groups: Vec::new(),
        }
    }
}
pub(super) struct Painter<'a, 'cx, 'draw> {
    pub cx: &'a mut Cx2d<'cx, 'draw>,
    pub gfx: &'a mut Graphics,
    pub shape: &'a mut TileShape,
    pub regular: &'a mut DrawText,
    pub bold: &'a mut DrawText,
    pub mono: &'a mut DrawText,
    pub line: &'a mut DrawLine,
    pub bitmap: &'a mut DrawBitmap,
    pub band: &'a mut DrawBand,
    pub transform: Mat4,
    pub group: usize,
}
fn color([r, g, b]: Rgb, a: u8) -> Vec4 {
    vec4(
        r as f32 / 255.,
        g as f32 / 255.,
        b as f32 / 255.,
        a as f32 / 255.,
    )
}
fn bounds(b: Box2) -> Rect {
    rect(b.x.into(), b.y.into(), b.w.into(), b.h.into())
}
impl Painter<'_, '_, '_> {
    fn face(&mut self, font: Font) -> &mut DrawText {
        let draw = match font.face {
            Face::Regular => &mut *self.regular,
            Face::Bold => &mut *self.bold,
            Face::Mono => &mut *self.mono,
        };
        draw.text_style.font_size = font.size * 0.75;
        draw.color = color(font.color, 255);
        draw
    }
    fn layout(
        &mut self,
        s: &str,
        f: Font,
        w: Option<f32>,
    ) -> std::rc::Rc<makepad_draw::text::layouter::LaidoutText> {
        self.face(f).text_style.line_spacing = if w.is_some() { 1.4 / 1.17 } else { 1. };
        let draw = match f.face {
            Face::Regular => &self.regular,
            Face::Bold => &self.bold,
            Face::Mono => &self.mono,
        };
        draw.layout(self.cx, 0., 0., w, w.is_some(), Align::default(), s)
    }
}
impl scene::Painter for Painter<'_, '_, '_> {
    fn scale(&self) -> f32 {
        self.cx.current_dpi_factor() as f32
    }
    fn rect(&mut self, b: Box2, c: Rgb, a: u8, r: f32) {
        self.shape.color = color(c, a);
        self.shape.radius = r.min(b.w / 2.).min(b.h / 2.);
        self.shape.stroke = 0.;
        self.shape.draw_abs(self.cx, bounds(b));
    }
    fn size(&mut self, s: &str, f: Font, w: Option<f32>) -> (f32, f32) {
        let dim = self.layout(s, f, w).size_in_lpxs;
        (dim.width, dim.height)
    }
    fn text(&mut self, b: Box2, s: &str, f: Font, wrap: bool) {
        if b.w <= 0. || b.h <= 0. {
            return;
        }
        let layout = self.layout(s, f, wrap.then_some(b.w));
        let draw = match f.face {
            Face::Regular => &mut *self.regular,
            Face::Bold => &mut *self.bold,
            Face::Mono => &mut *self.mono,
        };
        let walk = Walk::abs_rect(bounds(b));
        self.cx.begin_turtle(walk, Layout::default());
        draw.draw_walk_laidout(self.cx, walk, &layout);
        self.cx.end_turtle();
    }
    fn image(&mut self, b: Box2, id: ImageId, r: f32) {
        self.bitmap.radius = r;
        self.bitmap
            .draw_vars
            .set_texture(0, self.gfx.images.get(id));
        self.bitmap.draw_abs(self.cx, bounds(b));
    }
    fn path(&mut self, points: &[(f32, f32)], col: Rgb, stroke: f32, fill_to: Option<f32>) {
        let color = color(col, 255);
        if let Some(bottom) = fill_to {
            let top = points.iter().map(|pt| pt.1).fold(bottom, f32::min);
            for pair in points.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                self.band.left_y = a.1;
                self.band.right_y = b.1;
                self.band.color = color;
                self.band.draw_abs(
                    self.cx,
                    bounds(Box2::new(a.0, top, b.0 - a.0, bottom - top)),
                );
            }
        }
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            self.line.draw_line_abs(
                self.cx,
                dvec2(a.0.into(), a.1.into()),
                dvec2(b.0.into(), b.1.into()),
                color,
                stroke.into(),
            );
        }
    }
    fn clipped(&mut self, b: Box2, draw: impl FnOnce(&mut Self)) {
        self.cx
            .begin_turtle(Walk::abs_rect(bounds(b)), Layout::default());
        draw(self);
        self.cx.end_turtle();
    }
    fn rotated(&mut self, (x, y): (f32, f32), deg: f32, draw: impl FnOnce(&mut Self)) {
        let slot = self.group;
        self.group += 1;
        if slot == self.gfx.groups.len() {
            self.gfx.groups.push(DrawList2d::new(self.cx));
        }
        let old = self.transform;
        let rotate = Mat4::rotation(vec3(0., 0., deg.to_radians()));
        let local = Mat4::mul(
            &Mat4::translation(vec3(-x, -y, 0.)),
            &Mat4::mul(&rotate, &Mat4::translation(vec3(x, y, 0.))),
        );
        self.transform = Mat4::mul(&local, &old);
        let list = &mut self.gfx.groups[slot];
        list.begin_always(self.cx);
        list.set_view_transform(self.cx, &self.transform);
        draw(self);
        self.gfx.groups[slot].end(self.cx);
        self.transform = old;
    }
    fn cached(&mut self, i: usize, b: Box2, draw: impl FnOnce(&mut Self)) {
        let (list, last) = &mut self.gfx.static_lists[i];
        if list.begin_maybe(self.cx, *last != Some(b)).is_redrawing() {
            *last = Some(b);
            draw(self);
            self.gfx.static_lists[i].0.end(self.cx);
        }
    }
}
