#![allow(unsafe_code)]

use cranpose_ui_graphics::{BlendMode, Brush, CornerRadii, DrawPrimitive, Point, Rect, Stroke};

pub(crate) struct ShapeRunEntry<'a> {
    primitive: &'a DrawPrimitive,
    pub(crate) blend_mode: BlendMode,
    pub(crate) clip: Option<Rect>,
}

impl<'a> ShapeRunEntry<'a> {
    pub(crate) fn new(primitive: &'a DrawPrimitive, clip: Option<Rect>) -> Option<Self> {
        let (primitive, blend_mode) = match primitive {
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => (primitive.as_ref(), *blend_mode),
            other => (other, BlendMode::SrcOver),
        };
        match primitive {
            DrawPrimitive::Rect { .. }
            | DrawPrimitive::RoundRect { .. }
            | DrawPrimitive::Arc { .. } => Some(Self {
                primitive,
                blend_mode,
                clip,
            }),
            _ => None,
        }
    }

    pub(crate) fn primitive(&self) -> &'a DrawPrimitive {
        self.primitive
    }
}

#[allow(dead_code)]
fn admitted_payloads_are_sync() {
    fn ok<T: Sync>() {}
    ok::<Rect>();
    ok::<Brush>();
    ok::<Option<Stroke>>();
    ok::<CornerRadii>();
    ok::<Point>();
    ok::<f32>();
    ok::<BlendMode>();
}

// SAFETY: `new` is the only producer and admits only the `Rect`, `RoundRect`
// and `Arc` variants, whose payloads are all `Sync` (proven above); the `Rc`
// carried by other `DrawPrimitive` variants is never reachable through an
// entry. Shared access from the frame worker pool therefore never touches
// non-`Sync` data, and the barrier in `FrameWorkerPool::run` ends all worker
// access before the borrow does.
unsafe impl Send for ShapeRunEntry<'_> {}
unsafe impl Sync for ShapeRunEntry<'_> {}
