pub(crate) type Rgb = [u8; 3];
pub(crate) const WHITE: Rgb = [255; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Box2 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Box2 {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            w: w.max(0.),
            h: h.max(0.),
        }
    }
    pub fn inset(self, n: f32) -> Self {
        Self::new(self.x + n, self.y + n, self.w - 2. * n, self.h - 2. * n)
    }
    pub fn at(self, x: f32, y: f32, w: f32, h: f32) -> Self {
        Self::new(self.x + x, self.y + y, w, h)
    }
    pub fn center(self) -> (f32, f32) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Face {
    Regular,
    Bold,
    Mono,
}
#[derive(Clone, Copy)]
pub(crate) struct Font {
    pub size: f32,
    pub face: Face,
    pub color: Rgb,
}
impl Font {
    pub fn new(size: f32, face: Face, color: Rgb) -> Self {
        Self { size, face, color }
    }
    pub fn regular(size: f32, color: Rgb) -> Self {
        Self::new(size, Face::Regular, color)
    }
    pub fn bold(size: f32, color: Rgb) -> Self {
        Self::new(size, Face::Bold, color)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ImageId {
    Avatar(usize),
    Gif(usize),
    Spinner,
}

pub(crate) trait Painter: Sized {
    fn scale(&self) -> f32;
    fn rect(&mut self, area: Box2, color: Rgb, alpha: u8, radius: f32);
    fn image(&mut self, area: Box2, id: ImageId, radius: f32);
    fn size(&mut self, text: &str, font: Font, width: Option<f32>) -> (f32, f32);
    fn text(&mut self, area: Box2, text: &str, font: Font, wrap: bool);
    fn path(&mut self, points: &[(f32, f32)], color: Rgb, stroke: f32, fill_to: Option<f32>);
    fn clipped(&mut self, area: Box2, draw: impl FnOnce(&mut Self));
    fn rotated(&mut self, center: (f32, f32), degrees: f32, draw: impl FnOnce(&mut Self));
    fn cached(&mut self, slot: usize, area: Box2, draw: impl FnOnce(&mut Self));
}
