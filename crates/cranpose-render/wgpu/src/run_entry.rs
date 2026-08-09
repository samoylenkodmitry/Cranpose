//! The borrowed run-entry type shared by shape-run collection, emission and
//! replay.
//!
//! An entry borrows its [`DrawPrimitive`] instead of copying the shape
//! payload out of it. Collection used to re-read every field of a ~17k-entry
//! run each frame just to copy ~100 bytes per entry into a parallel-safe
//! enum; on a watch-class core that second streaming pass over the run was
//! measurable by itself, and the emit/replay consumers read the same fields
//! again anyway.
//!
//! `DrawPrimitive` as a type is `!Sync` — its `Text` variant holds `Rc` —
//! so a borrowed entry cannot derive `Send`/`Sync` even though a run never
//! stores such a variant. [`ShapeRunEntry::new`] is the only constructor and
//! admits only the `Rect`, `RoundRect` and `Arc` variants, whose payloads
//! are all `Sync`; the manual impls below rest on exactly that, which is why
//! this module is kept small enough for the invariant and the impls to share
//! a screen.
#![allow(unsafe_code)]

use cranpose_ui_graphics::{BlendMode, Brush, CornerRadii, DrawPrimitive, Point, Rect, Stroke};

/// One accumulated draw in a shape run: the blend-unwrapped shape primitive,
/// its blend mode (single-level `Blend` wrappers resolve at construction),
/// and the per-draw clip its graph node carried (`None` for every
/// [`cranpose_render_common::graph::DrawRunNode`] draw — a run node records
/// a whole canvas command, which never clips per primitive).
pub(crate) struct ShapeRunEntry<'a> {
    /// Private on purpose: the `Sync` impls below are sound only because
    /// [`ShapeRunEntry::new`] controls which variants can be stored here.
    primitive: &'a DrawPrimitive,
    pub(crate) blend_mode: BlendMode,
    pub(crate) clip: Option<Rect>,
}

impl<'a> ShapeRunEntry<'a> {
    /// The run-entry view of a primitive a shape run can carry: the plain
    /// shape variants and a single-level blend of one. Everything else
    /// (text, images, shadows, content markers, nested blends) returns
    /// `None`; callers flush the run and take the ordinary per-primitive
    /// path.
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

    /// The blend-unwrapped primitive: always one of `Rect`, `RoundRect` or
    /// `Arc`.
    pub(crate) fn primitive(&self) -> &'a DrawPrimitive {
        self.primitive
    }
}

/// Compile-time proof that every payload type reachable through an admitted
/// variant is `Sync`. If a field of `Rect`/`RoundRect`/`Arc` ever changes to
/// a non-`Sync` type, this stops compiling before the impls below can lie.
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
