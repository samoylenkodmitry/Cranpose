use cranpose_ui_graphics::{
    BrushRecord, GradientStopRecord, RecordTables, ShapeRecordBody, ShapeRecordCurve,
};

use super::{
    BRUSH_CHUNK, PLACEMENT_CHUNK, PlacementData, RECORD_CHUNK, RunBufferMode, RunDrawCall,
    STOP_CHUNK, painted_stops,
};
use crate::{run_geometry::ShapeFill, scene::Placement};

/// The CPU side of one arena chunk while a pass fills it.
#[derive(Default)]
pub(crate) struct ArenaStaging {
    pub(super) bodies: Vec<ShapeRecordBody>,
    pub(super) curves: Vec<ShapeRecordCurve>,
    pub(super) brushes: Vec<BrushRecord>,
    pub(super) stops: Vec<GradientStopRecord>,
    pub(super) placements: Vec<PlacementData>,
    pub(super) brush_map: Vec<u32>,
    painted: Vec<GradientStopRecord>,
    pub(super) draws: Vec<RunDrawCall>,
    pub(crate) fill: ShapeFill,
    pub(super) arcs: bool,
}

impl ArenaStaging {
    pub(super) fn clear(&mut self) {
        self.arcs = false;
        self.bodies.clear();
        self.curves.clear();
        self.brushes.clear();
        self.stops.clear();
        self.placements.clear();
        self.draws.clear();
        self.fill = ShapeFill::default();
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.bodies.capacity() * size_of::<ShapeRecordBody>()
            + self.curves.capacity() * size_of::<ShapeRecordCurve>()
            + self.brushes.capacity() * size_of::<BrushRecord>()
            + self.stops.capacity() * size_of::<GradientStopRecord>()
            + self.placements.capacity() * size_of::<PlacementData>()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    pub(super) fn fits(
        &self,
        mode: RunBufferMode,
        records: usize,
        brushes: usize,
        stops: usize,
    ) -> bool {
        if mode.storage {
            return true;
        }
        self.bodies.len() + records <= RECORD_CHUNK
            && self.brushes.len() + brushes <= BRUSH_CHUNK
            && self.stops.len() + stops <= STOP_CHUNK
            && self.placements.len() < PLACEMENT_CHUNK
    }

    pub(super) fn push_draw(&mut self, draw: RunDrawCall) {
        if self.draws.last_mut().is_some_and(|last| {
            last.absorb(
                draw.key,
                draw.band_class,
                draw.records.clone(),
                draw.occluders,
            )
        }) {
            return;
        }
        self.draws.push(draw);
    }

    pub(super) fn map_brush(
        &mut self,
        tables: &RecordTables,
        placement: &Placement,
        mode: RunBufferMode,
        source: usize,
    ) -> Option<u32> {
        let storage = mode.storage;
        let mapped = self.brush_map[source];
        if mapped != u32::MAX {
            return Some(mapped);
        }
        let brush = tables.brushes()[source];
        let stop_range = brush.stop_start as usize..(brush.stop_start + brush.stop_count) as usize;
        if !storage
            && (self.brushes.len() >= BRUSH_CHUNK
                || self.stops.len() + brush.stop_count as usize > STOP_CHUNK)
        {
            return None;
        }
        painted_stops(&tables.stops()[stop_range], placement, &mut self.painted);
        let stop_start = self.stops.len() as u32;
        self.stops.extend_from_slice(&self.painted);
        self.brushes.push(BrushRecord {
            stop_start,
            ..brush
        });
        let index = self.brushes.len() as u32;
        self.brush_map[source] = index;
        Some(index)
    }
}
