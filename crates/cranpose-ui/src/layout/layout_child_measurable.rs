//! A child of a layout node as the node's measure policy sees it: its
//! measure, intrinsic sizes and parent data, through the record its parent
//! keeps of it.

use std::rc::Rc;

use cranpose_ui_layout::{AlignmentLines, Constraints, FlexParentData, ParentData, PlaceTarget};

use super::{
    LayoutChildMeasureState,
    core::{Measurable, Placeable},
    read_cell,
};
use crate::{
    modifier::Size,
    widgets::nodes::{IntrinsicKind, LayoutParts},
};

pub(super) struct LayoutChildMeasurable {
    state: Rc<LayoutChildMeasureState>,
}

impl LayoutChildMeasurable {
    pub(super) fn new(state: Rc<LayoutChildMeasureState>) -> Self {
        Self { state }
    }

    // Runs `read` on the parent data of the record's bind, while the record
    // is bound to a pass.
    fn read_parent_data<R>(&self, read: impl FnOnce(&ParentData) -> R) -> Option<R> {
        if self.state.frame.builder.borrow().is_none() {
            return None;
        }
        self.state.parent_data.borrow().as_ref().map(read)
    }

    // How many state reads the pass that binds the child has kept.
    fn reads_seen(&self) -> usize {
        self.state
            .frame
            .builder
            .borrow()
            .as_ref()
            .map_or(0, |builder| builder.context.measure_reads.seen())
    }

    fn intrinsic(
        &self,
        kind: IntrinsicKind,
        constraints: Constraints,
        extent: fn(Size) -> f32,
    ) -> f32 {
        let state = &self.state;
        if state.frame.measures() {
            state.read_intrinsics.set(true);
        }
        let parts = read_cell(&state.parts, Clone::clone);
        let cache = parts.as_deref().map(LayoutParts::cache);
        if let Some(cache) = cache {
            cache.activate(state.cache_epoch.get().get());
        }
        if !state.force_remeasure.get()
            && let Some(value) = cache.and_then(|cache| cache.get_intrinsic(&kind))
        {
            return value;
        }
        let seen = self.reads_seen();
        let value = match state.layout_intrinsic(kind) {
            Some(value) => value,
            None => state
                .measure_cached(constraints)
                .map_or(0.0, |node| extent(node.size_for_parent())),
        };
        // A value that read a state holds only while the state does: the
        // measure that asked for it keeps the read and asks again, as
        // Compose, which keeps no intrinsic sizes, does.
        if let Some(cache) = cache
            && self.reads_seen() == seen
        {
            cache.store_intrinsic(kind, value);
        }
        value
    }
}

impl Measurable for LayoutChildMeasurable {
    fn measured_hold(&self) -> Option<(Size, cranpose_ui_layout::ConstraintsHold)> {
        read_cell(&self.state.measured, |measured| {
            let measured = measured.as_ref()?;
            let hold = read_cell(&self.state.parts, |parts| {
                parts.as_ref()?.cache().hold_of(measured)
            })?;
            Some((measured.size_for_parent(), hold))
        })
    }

    fn measure(&self, constraints: Constraints) -> Placeable {
        let state = &self.state;
        let measured = state.measure_cached(constraints);
        let (measured_size, size_for_parent) = measured.as_ref().map_or(
            (
                Size {
                    width: 0.0,
                    height: 0.0,
                },
                Size {
                    width: 0.0,
                    height: 0.0,
                },
            ),
            |measured| (measured.size, measured.size_for_parent()),
        );
        state.with_pass_layout_state(|layout_state, context| {
            layout_state.set_size(measured_size, context);
        });
        let alignment_lines = measured
            .as_ref()
            .map_or_else(AlignmentLines::default, |node| {
                node.alignment_lines_for_parent()
            });
        state.measured.set(measured);

        Placeable::with_place_target(
            size_for_parent.width,
            size_for_parent.height,
            state.node_id,
            Rc::clone(&self.state) as Rc<dyn PlaceTarget>,
        )
        .with_alignment_lines(alignment_lines)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MinWidth(height),
            Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: height,
                max_height: height,
            },
            |size| size.width,
        )
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MaxWidth(height),
            Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: 0.0,
                max_height: height,
            },
            |size| size.width,
        )
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MinHeight(width),
            Constraints {
                min_width: width,
                max_width: width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            },
            |size| size.height,
        )
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MaxHeight(width),
            Constraints {
                min_width: 0.0,
                max_width: width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            },
            |size| size.height,
        )
    }

    fn flex_parent_data(&self) -> Option<FlexParentData> {
        self.read_parent_data(|data| {
            data.has_weight()
                .then(|| FlexParentData::new(data.weight, data.fill))
        })
        .flatten()
    }

    fn parent_data(&self) -> ParentData {
        self.read_parent_data(ParentData::clone).unwrap_or_default()
    }
}
