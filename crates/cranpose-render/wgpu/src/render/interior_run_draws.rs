use smallvec::SmallVec;

use super::{PlannedRunDraw, ShapePipelineKey};
use crate::run_store::RunDrawCall;

/// The draw calls laying a batch's opaque interiors down: last first, so
/// they go down front to back, one call over each stretch of neighbouring
/// draws that share the interior pipeline, cut to the span from the stretch's
/// first draw holding an occluder to its last. The draws between ride along:
/// the pipeline lays any record's exact interior down and skips the rest, so
/// taking them costs a few vertices rather than a call, and it shades only a
/// record's quad, the first two triangles of every band class's pattern.
pub(crate) fn interior_run_draws(draws: &[RunDrawCall]) -> SmallVec<[PlannedRunDraw; 4]> {
    struct Stretch {
        key: ShapePipelineKey,
        start: u32,
        span: Option<PlannedRunDraw>,
    }
    let mut planned: SmallVec<[PlannedRunDraw; 4]> = SmallVec::new();
    let mut stretch: Option<Stretch> = None;
    // A stored run drawn while an arena chunk stays open sits among the
    // chunk's draws, its records in tables of its own: its draws lay their
    // interiors with its batch and leave the chunk's stretches whole.
    let tier = draws.first().map(|draw| draw.key.tier);
    for draw in draws
        .iter()
        .rev()
        .filter(|draw| Some(draw.key.tier) == tier)
    {
        let Some(key) = draw.key.interior() else {
            planned.extend(stretch.take().and_then(|open| open.span));
            continue;
        };
        if stretch
            .as_ref()
            .is_some_and(|open| open.key != key || open.start != draw.records.end)
        {
            planned.extend(stretch.take().and_then(|open| open.span));
        }
        let open = stretch.get_or_insert(Stretch {
            key,
            start: draw.records.end,
            span: None,
        });
        open.start = draw.records.start;
        if !draw.occluders {
            continue;
        }
        match &mut open.span {
            Some(span) => span.records.start = draw.records.start,
            None => {
                open.span = Some(PlannedRunDraw {
                    key,
                    band_class: draw.band_class,
                    indices: 0..cranpose_ui_graphics::strip_indices(1),
                    records: draw.records.clone(),
                });
            }
        }
    }
    planned.extend(stretch.and_then(|open| open.span));
    planned
}
