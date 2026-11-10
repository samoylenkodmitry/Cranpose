//! Pictures that paint themselves: bitmaps, one of which a signal picks, and
//! the chat's SVG spinner.

use std::{cell::Cell, rc::Rc, sync::Arc};

use floem::{
    View, ViewId,
    context::{PaintCx, UpdateCx},
    kurbo::{Rect, Size},
    peniko::{Blob, Brush, Format, Image},
    reactive::create_effect,
};
use floem_renderer::{
    Img, Renderer as _, Svg,
    usvg::{Options, Tree},
};

/// A bitmap of tightly packed RGBA bytes.
pub fn bitmap(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    Image::new(Blob::new(Arc::new(rgba)), Format::Rgba8, width, height)
}

/// The size a view's layout gave it.
fn bounds(id: ViewId) -> Rect {
    let layout = id.get_layout().unwrap_or_default();
    Size::new(f64::from(layout.size.width), f64::from(layout.size.height)).to_rect()
}

/// One of several bitmaps, the one `pick` names: a change of the pick paints
/// again and lays out nothing.
pub struct Picture {
    id: ViewId,
    images: Rc<Vec<Image>>,
    current: Cell<usize>,
}

pub fn picture(images: Rc<Vec<Image>>, pick: impl Fn() -> usize + 'static) -> Picture {
    let id = ViewId::new();
    create_effect(move |_| id.update_state(pick()));
    Picture {
        id,
        images,
        current: Cell::new(0),
    }
}

impl View for Picture {
    fn id(&self) -> ViewId {
        self.id
    }

    fn update(&mut self, _cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        if let Ok(pick) = state.downcast::<usize>() {
            self.current.set(*pick);
            self.id.request_paint();
        }
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        if let Some(image) = self.images.get(self.current.get()) {
            cx.draw_img(
                Img {
                    img: image.clone(),
                    hash: &[],
                },
                bounds(self.id),
            );
        }
    }
}

/// An SVG in its own colors.
pub struct Vector {
    id: ViewId,
    tree: Option<Tree>,
}

pub fn vector(svg: &str) -> Vector {
    let tree = Tree::from_str(svg, &Options::default())
        .map_err(|error| log::warn!("chat spinner: {error}"))
        .ok();
    Vector {
        id: ViewId::new(),
        tree,
    }
}

impl View for Vector {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        if let Some(tree) = &self.tree {
            cx.draw_svg(Svg { tree, hash: &[] }, bounds(self.id), None::<&Brush>);
        }
    }
}
