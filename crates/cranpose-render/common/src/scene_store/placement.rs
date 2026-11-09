// Where a record sits in its parent's layer: its placement, its origin and
// its content offset, as the store keeps and reads them.

use cranpose_core::RecordId;
use cranpose_ui::Point;
use cranpose_ui_graphics::ProjectiveTransform;

use super::{RecordKind, SceneStore, TransformId};

/// Where a record sits in its parent's layer: at a point, where layout
/// placed a record that nothing transforms, or by a transform. A transform
/// that only moves a record is a point, so two equal placements compare
/// equal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// At the point, past the parent's content offset.
    At(Point),
    /// By the transform, which takes the record's space to its parent's.
    By(ProjectiveTransform),
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// The offset of the content box of `id` from its origin.
    #[inline]
    pub fn content_offset(&self, id: RecordId) -> Point {
        let [x, y] = self.warm[id.index()].content_offset;
        Point::new(x, y)
    }

    /// Sets the offset of the content box of `id`.
    #[inline]
    pub fn set_content_offset(&mut self, id: RecordId, offset: Point) {
        let index = id.index();
        let offset = [offset.x, offset.y];
        if !super::records::same_pair(self.warm[index].content_offset, offset) {
            self.warm[index].content_offset = offset;
            self.stale(index);
            self.moved(index);
        }
    }

    /// Where layout placed `id` in its parent's content, before the
    /// parent's content offset.
    #[inline]
    pub fn origin_in_parent(&self, id: RecordId) -> Point {
        let [x, y] = self.geo[id.index()].placement;
        Point::new(x, y)
    }

    /// The top left of `id` in its parent's layer: its placement and the
    /// parent's content offset.
    #[inline]
    pub fn origin(&self, id: RecordId) -> Point {
        let index = id.index();
        match self.tree[index].transform {
            Some(transform) => self.transforms[transform.index()]
                .as_translation()
                .unwrap_or_else(|| self.placed_origin(index)),
            None => self.placed_origin(index),
        }
    }

    // Where the placement of the record at `index` puts it in its parent's
    // layer, past the parent's content offset.
    #[inline]
    fn placed_origin(&self, index: usize) -> Point {
        let [x, y] = self.geo[index].placement;
        let offset = self.parent_content_offset(index);
        match self.tree[index].kind {
            RecordKind::Leaf => Point::new(x + offset.x, y + offset.y),
            RecordKind::Layer | RecordKind::Wrapper => {
                Point::new(x + 0.0 + offset.x, y + 0.0 + offset.y)
            }
            // A draw group lies in its node's own space, where the content
            // offset does not move it.
            RecordKind::DrawGroup => Point::new(x, y),
        }
    }

    #[inline]
    fn parent_content_offset(&self, index: usize) -> Point {
        self.tree[index]
            .parent
            .map(|parent| self.content_offset(parent))
            .unwrap_or_default()
    }

    #[expect(
        clippy::inline_always,
        reason = "the collect walk and the update call it for every record; out of line the gauntlet frame took 2% more instructions"
    )]
    /// The transform from `id` to its parent's content.
    #[inline(always)]
    pub fn transform_to_parent(&self, id: RecordId) -> ProjectiveTransform {
        match self.tree[id.index()].transform {
            Some(transform) => self.transforms[transform.index()],
            None => {
                let origin = self.placed_origin(id.index());
                ProjectiveTransform::translation(origin.x, origin.y)
            }
        }
    }

    /// Where `id` sits in its parent's layer.
    #[inline]
    pub fn placement(&self, id: RecordId) -> Placement {
        match self.tree[id.index()].transform {
            Some(transform) => Placement::By(self.transforms[transform.index()]),
            None => Placement::At(self.placed_origin(id.index())),
        }
    }

    /// Places `id` at `origin_in_parent` in its parent's content by
    /// `transform`, or by a translation there alone, past the parent's
    /// content offset, for `None`. A transform that only moves the record
    /// there takes no entry of its own. The parent's content offset is set
    /// before.
    pub fn set_placement(
        &mut self,
        id: RecordId,
        origin_in_parent: Point,
        transform: Option<ProjectiveTransform>,
    ) {
        let index = id.index();
        let placement = [origin_in_parent.x, origin_in_parent.y];
        let moved = !super::records::same_pair(self.geo[index].placement, placement);
        self.geo[index].placement = placement;
        // An entry holds a transform that does more than place the record.
        let transform = transform.filter(|transform| {
            let origin = self.placed_origin(index);
            !super::records::same_bits(
                *transform,
                ProjectiveTransform::translation(origin.x, origin.y),
            )
        });
        let entry = self.tree[index].transform;
        let unchanged = !moved
            && match (entry, transform) {
                (Some(entry), Some(transform)) => {
                    super::records::same_bits(self.transforms[entry.index()], transform)
                }
                (None, None) => true,
                (Some(_), None) | (None, Some(_)) => false,
            };
        if unchanged {
            return;
        }
        self.moved(index);
        if let Some(parent) = self.tree[index].parent {
            self.stale(parent.index());
        }
        match (transform, entry) {
            (None, Some(old)) => {
                self.tree[index].transform = None;
                self.transforms.remove(old.index());
            }
            (None, None) => {}
            (Some(transform), Some(old)) => self.transforms[old.index()] = transform,
            (Some(transform), None) => {
                let entry = TransformId::from_index(self.transforms.insert(transform));
                self.tree[index].transform = Some(entry);
            }
        }
    }
}
