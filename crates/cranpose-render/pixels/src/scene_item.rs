//! The items a raster scene keeps a list of, one list a kind: what pushing
//! one takes, and the item borrowed whatever its kind.

use cranpose_ui_graphics::Point;

use crate::{
    draw::RenderItem,
    scene::{DrawShape, ImageDraw, ItemCounts, RasterScene, ShadowDraw, TextDraw},
};

// An item of a scene, borrowed from its list.
#[derive(Clone, Copy)]
pub(crate) enum ItemRef<'a> {
    Shape(&'a DrawShape),
    Image(&'a ImageDraw),
    Text(&'a TextDraw),
    Shadow(&'a ShadowDraw),
}

pub(crate) trait SceneItem: Sized {
    // The draw order's entry for the item at `index` of this kind's list.
    fn item(index: usize) -> RenderItem;
    // The index into this kind's list that `item` stands for, when it is of
    // this kind.
    fn index_of(item: RenderItem) -> Option<usize>;
    // How many items of this kind `counts` holds.
    fn count(counts: &mut ItemCounts) -> &mut usize;
    // This kind's list in `scene`, and the scene's draw order.
    fn lists(scene: &mut RasterScene) -> (&mut Vec<Self>, &mut Vec<RenderItem>);
    // Snaps the item to the pixel grid as its layer, whose snap anchor is
    // `anchor`, lands on it.
    fn snap_to(&mut self, anchor: Point);
    fn as_ref(&self) -> ItemRef<'_>;
}

macro_rules! scene_item {
    ($item:ident, $variant:ident, $list:ident) => {
        impl SceneItem for $item {
            fn item(index: usize) -> RenderItem {
                RenderItem::$variant(index)
            }

            fn index_of(item: RenderItem) -> Option<usize> {
                match item {
                    RenderItem::$variant(index) => Some(index),
                    _ => None,
                }
            }

            fn count(counts: &mut ItemCounts) -> &mut usize {
                &mut counts.$list
            }

            fn lists(scene: &mut RasterScene) -> (&mut Vec<Self>, &mut Vec<RenderItem>) {
                (&mut scene.$list, &mut scene.order)
            }

            fn snap_to(&mut self, anchor: Point) {
                self.snap_anchor = Some(anchor);
            }

            fn as_ref(&self) -> ItemRef<'_> {
                ItemRef::$variant(self)
            }
        }
    };
}

scene_item!(DrawShape, Shape, shapes);
scene_item!(ImageDraw, Image, images);
scene_item!(TextDraw, Text, texts);
scene_item!(ShadowDraw, Shadow, shadows);
