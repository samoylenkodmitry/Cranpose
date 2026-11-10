//! Content that never changes, drawn once into a bitmap and shown by a view.

use floem::{
    View, ViewId,
    context::PaintCx,
    kurbo::Rect,
    peniko::{Color, Image},
};
use floem_renderer::{Img, Renderer as _};

use crate::raster::Raster;

type Draw = Box<dyn FnOnce(&mut Raster)>;

pub struct Layer {
    id: ViewId,
    size: (f32, f32),
    background: Color,
    draw: Option<Draw>,
    image: Option<Image>,
}

/// A view of `size` that shows what `draw` puts on a bitmap over `background`.
/// The bitmap has a pixel to each pixel of the screen, which the first paint
/// knows.
pub fn layer(
    size: (f32, f32),
    background: Color,
    draw: impl FnOnce(&mut Raster) + 'static,
) -> Layer {
    Layer {
        id: ViewId::new(),
        size,
        background,
        draw: Some(Box::new(draw)),
        image: None,
    }
}

impl View for Layer {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        if let Some(draw) = self.draw.take() {
            let mut raster = Raster::new(self.size, cx.scale() as f32, self.background);
            draw(&mut raster);
            self.image = Some(raster.into_image());
        }
        if let Some(image) = &self.image {
            cx.draw_img(
                Img {
                    img: image.clone(),
                    hash: &[],
                },
                Rect::new(0.0, 0.0, f64::from(self.size.0), f64::from(self.size.1)),
            );
        }
    }
}
