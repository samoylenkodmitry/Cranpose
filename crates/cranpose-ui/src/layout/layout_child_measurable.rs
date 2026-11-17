//! A child of a layout node as the node's measure policy sees it: its
//! measure, intrinsic sizes and parent data, through the record its parent
//! keeps of it in the child's parts.

use std::{ops::Deref, rc::Rc};

use cranpose_ui_layout::{AlignmentLines, Constraints, FlexParentData, ParentData, PlaceTarget};

use super::{
    core::{Measurable, Placeable},
    read_cell,
};
use crate::{
    modifier::Size,
    widgets::nodes::{IntrinsicKind, LayoutParts},
};

// A parent's entry for one child: the child's parts, which hold the
// parent's record of the child. The parent's measure policy measures it.
pub(super) struct LayoutChildMeasurable {
    parts: Rc<LayoutParts>,
}

impl Deref for LayoutChildMeasurable {
    type Target = Rc<LayoutParts>;

    fn deref(&self) -> &Self::Target {
        &self.parts
    }
}

impl LayoutChildMeasurable {
    pub(super) fn new(parts: Rc<LayoutParts>) -> Self {
        Self { parts }
    }

    // Runs `read` on the parent data of the record's bind, while the record
    // is bound to a pass.
    fn read_parent_data<R>(&self, read: impl FnOnce(&ParentData) -> R) -> Option<R> {
        let record = self.parts.record();
        record.with_builder(|_| ())?;
        Some(read_cell(&record.parent_data, |data| data.read(read)))
    }

    // How many state reads the pass that binds the child has kept.
    fn reads_seen(&self) -> usize {
        self.parts
            .record()
            .with_builder(|builder| builder.context.measure_reads.seen())
            .unwrap_or(0)
    }

    fn intrinsic(
        &self,
        kind: IntrinsicKind,
        constraints: Constraints,
        extent: fn(Size) -> f32,
    ) -> f32 {
        let parts = &self.parts;
        let record = parts.record();
        if record.measures() {
            record.read_intrinsics.set(true);
        }
        let cache = parts.cache();
        cache.activate(record.cache_epoch.get().get());
        if !record.force_remeasure.get()
            && let Some(value) = cache.get_intrinsic(&kind)
        {
            return value;
        }
        let seen = self.reads_seen();
        let value = match parts.layout_intrinsic(kind) {
            Some(value) => value,
            None => parts
                .measure_cached(constraints)
                .map_or(0.0, |node| extent(node.size_for_parent())),
        };
        // A value that read a state holds only while the state does: the
        // measure that asked for it keeps the read and asks again, as
        // Compose, which keeps no intrinsic sizes, does.
        if self.reads_seen() == seen {
            cache.store_intrinsic(kind, value);
        }
        value
    }
}

impl Measurable for LayoutChildMeasurable {
    fn measured_hold(&self) -> Option<(Size, cranpose_ui_layout::ConstraintsHold)> {
        read_cell(&self.parts.record().measured, |measured| {
            let measured = measured.as_ref()?;
            let hold = self.parts.cache().hold_of(measured)?;
            Some((measured.size_for_parent(), hold))
        })
    }

    fn measure(&self, constraints: Constraints) -> Placeable {
        let parts = &self.parts;
        let measured = parts.measure_cached(constraints);
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
        let child_id = parts.record().child_id();
        parts.with_pass_layout_state(|layout_state, context| {
            layout_state.set_size(Some(child_id), measured_size, context);
        });
        let alignment_lines = measured
            .as_ref()
            .map_or_else(AlignmentLines::default, |node| {
                node.alignment_lines_for_parent()
            });
        parts.record().measured.set(measured);

        Placeable::with_place_target(
            size_for_parent.width,
            size_for_parent.height,
            child_id,
            Rc::clone(&self.parts) as Rc<dyn PlaceTarget>,
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
