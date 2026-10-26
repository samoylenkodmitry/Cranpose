//! Groups of draws a [`DrawScope`](crate::DrawScope) moves, scales, turns or
//! cuts as one block, as Compose's `withTransform` does.

use crate::{
    DrawPrimitive, GraphicsLayer, Path, Point, Rect, Size, TransformOrigin,
    layer_transform::layer_transform_to_parent,
};

/// How a group of draws lands in the scope that holds it: scaled, then
/// turned about the group's origin, then moved, as a [`GraphicsLayer`] with
/// its transform origin at the top left places its content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawGroup {
    /// Where the group's origin lands in the holding scope.
    pub translation: Point,
    /// Horizontal scale.
    pub scale_x: f32,
    /// Vertical scale.
    pub scale_y: f32,
    /// Clockwise turn on screen, in degrees.
    pub rotation: f32,
    /// The group's scope rect in its own coordinates: what its draws see as
    /// the scope, and what `clip` cuts to.
    pub bounds: Rect,
    /// Whether nothing the group draws outside `bounds` shows.
    pub clip: bool,
    /// Whether the group draws into a surface of its own before it is
    /// composited, so a mask drawn last in the group cuts only its draws.
    pub isolated: bool,
}

impl DrawGroup {
    /// A group that draws in the holding scope's coordinates.
    pub fn plain(bounds: Rect) -> Self {
        Self {
            translation: Point::default(),
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
            bounds,
            clip: false,
            isolated: false,
        }
    }

    /// A [`DrawGroup::plain`] group cut to `bounds`.
    pub fn clipped(bounds: Rect) -> Self {
        Self {
            clip: true,
            ..Self::plain(bounds)
        }
    }

    /// A group of `size` moved by `dx`, `dy`.
    pub(crate) fn moved(dx: f32, dy: f32, size: Size) -> Self {
        Self {
            translation: Point::new(dx, dy),
            ..Self::plain(Rect::from_size(size))
        }
    }

    /// A group that cuts its draws to the mask drawn last in it, over `rect`.
    pub(crate) fn masked(rect: Rect) -> Self {
        Self {
            isolated: true,
            ..Self::clipped(rect)
        }
    }

    /// A group of `size` scaled and turned about `pivot`, a point of the
    /// holding scope that stays where it is.
    pub(crate) fn about(
        pivot: Point,
        scale_x: f32,
        scale_y: f32,
        rotation: f32,
        size: Size,
    ) -> Self {
        let turned = Self {
            scale_x,
            scale_y,
            rotation,
            ..Self::plain(Rect::from_size(size))
        };
        let moved = turned.transform_to_parent().map_point(pivot);
        Self {
            translation: Point::new(pivot.x - moved.x, pivot.y - moved.y),
            ..turned
        }
    }

    /// Whether the group scales or turns its draws, beyond moving them.
    pub fn scales_or_turns(&self) -> bool {
        self.scale_x != 1.0 || self.scale_y != 1.0 || self.rotation != 0.0
    }

    /// The graphics layer that places the group's draws as the group does.
    pub fn layer(&self) -> GraphicsLayer {
        GraphicsLayer {
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            rotation_z: self.rotation,
            translation_x: self.translation.x,
            translation_y: self.translation.y,
            transform_origin: TransformOrigin::new(0.0, 0.0),
            ..GraphicsLayer::DEFAULT
        }
    }

    /// Maps the group's coordinates to the holding scope's.
    pub fn transform_to_parent(&self) -> crate::ProjectiveTransform {
        let origin = Rect {
            x: 0.0,
            y: 0.0,
            ..self.bounds
        };
        layer_transform_to_parent(origin, Point::default(), &self.layer())
    }

    /// The group moved by `dx`, `dy` in the holding scope, its draws with it.
    pub fn translate(self, dx: f32, dy: f32) -> Self {
        Self {
            translation: Point::new(self.translation.x + dx, self.translation.y + dy),
            ..self
        }
    }
}

/// A group's start or end among recorded primitives. The primitives between
/// a start and its end are in the group's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupMarker {
    /// The group's draws start here.
    Start(DrawGroup),
    /// The group opened last ends here.
    End,
}

/// One step of [`DrawScope::with_transform`](crate::DrawScope::with_transform),
/// as Compose's `DrawTransform` calls. Each step applies inside the steps
/// before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawTransform<'a> {
    /// Moves the draws by `dx`, `dy`.
    Translate {
        /// Horizontal move, in logical pixels.
        dx: f32,
        /// Vertical move, in logical pixels.
        dy: f32,
    },
    /// Scales the draws about `pivot`, which stays where it is.
    Scale {
        /// Horizontal scale.
        scale_x: f32,
        /// Vertical scale.
        scale_y: f32,
        /// The point of the scope the scale keeps in place.
        pivot: Point,
    },
    /// Turns the draws clockwise about `pivot`.
    Rotate {
        /// The turn, in degrees.
        degrees: f32,
        /// The point of the scope the turn keeps in place.
        pivot: Point,
    },
    /// Shrinks the scope by the four insets: the draws move by `left` and
    /// `top` and see the smaller size. Nothing is cut.
    Inset {
        /// Inset from the left edge.
        left: f32,
        /// Inset from the top edge.
        top: f32,
        /// Inset from the right edge.
        right: f32,
        /// Inset from the bottom edge.
        bottom: f32,
    },
    /// Cuts the draws to `rect`, in scope coordinates.
    ClipRect(Rect),
    /// Cuts the draws to the fill of a path, in scope coordinates.
    ClipPath(&'a Path),
}

/// Moves recorded primitives by `dx`, `dy` as one block: the primitives
/// outside every group and the outermost groups move, and what a group
/// holds moves with its group.
pub fn translate_recorded(
    primitives: impl IntoIterator<Item = DrawPrimitive>,
    dx: f32,
    dy: f32,
) -> impl Iterator<Item = DrawPrimitive> {
    let mut depth = 0u32;
    primitives
        .into_iter()
        .map(move |primitive| match primitive {
            DrawPrimitive::Group(GroupMarker::Start(group)) => {
                depth += 1;
                if depth == 1 {
                    DrawPrimitive::Group(GroupMarker::Start(group.translate(dx, dy)))
                } else {
                    primitive
                }
            }
            DrawPrimitive::Group(GroupMarker::End) => {
                depth = depth.saturating_sub(1);
                primitive
            }
            primitive if depth == 0 => primitive.translate(dx, dy),
            primitive => primitive,
        })
}

#[cfg(test)]
#[path = "tests/draw_group_tests.rs"]
mod tests;
