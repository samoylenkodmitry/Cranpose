use std::rc::Rc;

use cranpose_foundation::PointerEvent;
use cranpose_ui::{Brush, DrawCommand, DrawCommandFn, LayoutNodeData, ModifierNodeSlices};
use cranpose_ui_graphics::{
    BlendMode, Color, ColorFilter, CommandRecording, CompositingStrategy, CornerRadii,
    DrawPrimitive, FinishedRecording, GraphicsLayer, Point, RoundedCornerShape, Size,
};

use crate::layer_transform::{layer_scale_x, layer_scale_y, layer_uniform_scale};

pub struct NodeStyle {
    pub padding: cranpose_ui_graphics::EdgeInsets,
    pub background: Option<Color>,
    pub click_actions: Vec<Rc<dyn Fn(Point)>>,
    pub shape: Option<RoundedCornerShape>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub draw_commands: Vec<DrawCommand>,
    pub graphics_layer: Option<GraphicsLayer>,
    pub clip_to_bounds: bool,
}

impl NodeStyle {
    pub fn from_layout_node(data: &LayoutNodeData) -> Self {
        let resolved = data.resolved_modifiers;
        let slices: &ModifierNodeSlices = data.modifier_slices();
        let pointer_inputs = slices.pointer_inputs().to_vec();

        Self {
            padding: resolved.padding(),
            background: None,
            click_actions: slices.click_handlers().to_vec(),
            shape: None,
            pointer_inputs,
            draw_commands: slices.draw_commands().to_vec(),
            graphics_layer: slices.graphics_layer(),
            clip_to_bounds: slices.clip_to_bounds(),
        }
    }
}

pub fn combine_layers(
    current: GraphicsLayer,
    modifier_layer: Option<GraphicsLayer>,
) -> GraphicsLayer {
    if let Some(layer) = modifier_layer {
        GraphicsLayer {
            alpha: (current.alpha * layer.alpha).clamp(0.0, 1.0),
            scale: current.scale * layer.scale,
            scale_x: current.scale_x * layer.scale_x,
            scale_y: current.scale_y * layer.scale_y,
            rotation_x: current.rotation_x + layer.rotation_x,
            rotation_y: current.rotation_y + layer.rotation_y,
            rotation_z: current.rotation_z + layer.rotation_z,
            camera_distance: layer.camera_distance,
            transform_origin: layer.transform_origin,
            translation_x: current.translation_x + layer.translation_x,
            translation_y: current.translation_y + layer.translation_y,
            shadow_elevation: layer.shadow_elevation,
            ambient_shadow_color: layer.ambient_shadow_color,
            spot_shadow_color: layer.spot_shadow_color,
            shape: layer.shape,
            clip: current.clip || layer.clip,
            color_filter: compose_color_filters(current.color_filter, layer.color_filter),
            compositing_strategy: layer.compositing_strategy,
            blend_mode: layer.blend_mode,
            // render_effect is NOT inherited — it applies only to this layer's subtree
            render_effect: layer.render_effect,
            // backdrop_effect is NOT inherited — it applies only to this node's backdrop.
            backdrop_effect: layer.backdrop_effect,
        }
    } else {
        GraphicsLayer {
            compositing_strategy: CompositingStrategy::Auto,
            blend_mode: BlendMode::SrcOver,
            render_effect: None,
            backdrop_effect: None,
            ..current
        }
    }
}

pub use crate::graph::quad_bounds;

pub fn apply_layer_to_color(color: Color, layer: &GraphicsLayer) -> Color {
    apply_color_filter_to_color(
        Color(
            color.0,
            color.1,
            color.2,
            (color.3 * layer.alpha).clamp(0.0, 1.0),
        ),
        layer.color_filter,
    )
}

fn apply_color_filter_to_color(color: Color, filter: Option<ColorFilter>) -> Color {
    match filter {
        Some(filter) => {
            let [r, g, b, a] = filter.apply_rgba([color.0, color.1, color.2, color.3]);
            Color(r, g, b, a)
        }
        None => color,
    }
}

pub fn compose_color_filters(
    base: Option<ColorFilter>,
    overlay: Option<ColorFilter>,
) -> Option<ColorFilter> {
    match (base, overlay) {
        (None, None) => None,
        (Some(filter), None) | (None, Some(filter)) => Some(filter),
        (Some(filter), Some(next)) => Some(filter.compose(next)),
    }
}

pub fn apply_layer_to_brush(brush: Brush, layer: &GraphicsLayer) -> Brush {
    // The overwhelmingly common case — full-alpha layer, no filter, unit
    // scale — leaves every brush untouched; a scene of thousands of shape
    // draws per frame should not rebuild its colors to discover that.
    if layer.alpha == 1.0
        && layer.color_filter.is_none()
        && layer_scale_x(layer) == 1.0
        && layer_scale_y(layer) == 1.0
    {
        return brush;
    }
    let scale_x = layer_scale_x(layer);
    let scale_y = layer_scale_y(layer);
    let uniform_scale = layer_uniform_scale(layer);

    match brush {
        Brush::Solid(color) => Brush::solid(apply_layer_to_color(color, layer)),
        Brush::LinearGradient {
            colors,
            stops,
            mut start,
            mut end,
            tile_mode,
        } => {
            start.x *= scale_x;
            start.y *= scale_y;
            end.x *= scale_x;
            end.y *= scale_y;
            Brush::LinearGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                stops,
                start,
                end,
                tile_mode,
            }
        }
        Brush::RadialGradient {
            colors,
            stops,
            mut center,
            mut radius,
            tile_mode,
        } => {
            center.x *= scale_x;
            center.y *= scale_y;
            radius *= uniform_scale;
            Brush::RadialGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                stops,
                center,
                radius,
                tile_mode,
            }
        }
        Brush::SweepGradient {
            colors,
            stops,
            mut center,
        } => {
            center.x *= scale_x;
            center.y *= scale_y;
            Brush::SweepGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                stops,
                center,
            }
        }
    }
}

/// A layer-resolved brush, split at the solid/gradient boundary.
///
/// The per-frame shape emit produces one of these for every draw: the solid
/// case — effectively all of a heavy animated scene — carries its color
/// inline, so emitting a shape neither clones a `Brush` nor leaves an enum
/// with heap-carrying variants for frame teardown to walk. Only the rare
/// gradient still travels as a cloned [`Brush`].
#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedBrush {
    Solid(Color),
    /// A non-solid brush (gradients), already layer-resolved.
    Other(Brush),
}

impl ResolvedBrush {
    pub fn from_brush(brush: Brush) -> Self {
        match brush {
            Brush::Solid(color) => Self::Solid(color),
            other => Self::Other(other),
        }
    }

    /// The plain `Brush` this resolved form stands for — same values,
    /// reassembled for consumers that keep speaking `Brush`.
    pub fn into_brush(self) -> Brush {
        match self {
            Self::Solid(color) => Brush::Solid(color),
            Self::Other(brush) => brush,
        }
    }
}

/// [`apply_layer_to_brush`] without the solid-brush clone: the borrowed
/// brush's color is copied (or layer-adjusted) inline, and only gradients
/// are cloned. Produces exactly the values `apply_layer_to_brush` would —
/// both branches below mirror its fast path and its `Solid` arm verbatim.
pub fn resolve_layer_brush(brush: &Brush, layer: &GraphicsLayer) -> ResolvedBrush {
    match brush {
        Brush::Solid(color) => {
            if layer.alpha == 1.0
                && layer.color_filter.is_none()
                && layer_scale_x(layer) == 1.0
                && layer_scale_y(layer) == 1.0
            {
                ResolvedBrush::Solid(*color)
            } else {
                ResolvedBrush::Solid(apply_layer_to_color(*color, layer))
            }
        }
        other => ResolvedBrush::Other(apply_layer_to_brush(other.clone(), layer)),
    }
}

pub fn scale_corner_radii(radii: CornerRadii, scale: f32) -> CornerRadii {
    CornerRadii {
        top_left: radii.top_left * scale,
        top_right: radii.top_right * scale,
        bottom_right: radii.bottom_right * scale,
        bottom_left: radii.bottom_left * scale,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrawPlacement {
    Behind,
    Overlay,
}

pub fn primitives_for_placement(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
) -> Vec<DrawPrimitive> {
    primitives_for_placement_reusing(command, placement, size, Vec::new())
}

/// [`primitives_for_placement`] recording into `storage` the caller retains
/// between frames. A command that re-records every frame keeps one buffer
/// whose capacity was earned on earlier frames; only the rare marker-bearing
/// recording pays for a second output vector.
pub fn primitives_for_placement_reusing(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
    storage: Vec<DrawPrimitive>,
) -> Vec<DrawPrimitive> {
    primitives_for_placement_retained(
        command,
        placement,
        size,
        CommandRecording::default(),
        storage,
    )
    .0
}

/// The full retained form: the compact recording buffers come from and
/// return to the caller alongside the materialized primitives, so a command
/// re-recording every frame allocates nothing in the steady state.
pub fn primitives_for_placement_retained(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
    recording: CommandRecording,
    storage: Vec<DrawPrimitive>,
) -> (Vec<DrawPrimitive>, CommandRecording) {
    let mut no_replay = None;
    let (primitives, recording, _) = primitives_for_placement_verified(
        command,
        placement,
        size,
        recording,
        storage,
        &mut no_replay,
        None,
    );
    (primitives, recording)
}

/// [`primitives_for_placement_retained`] with per-command similarity
/// verification: when `replay` carries the command's state, the freshly
/// recorded compact form advances it BEFORE materialization, yielding the
/// frame's retained/dynamic span structure in primitive space. Rendering
/// still materializes everything — consuming the spans (and skipping
/// materialization for retained ones) is the renderer-side half of the
/// retention work. The frame is only returned for marker-free recordings:
/// content splitting reindexes the primitive vector, and no game-scale
/// command records content markers.
pub fn primitives_for_placement_verified(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
    recording: CommandRecording,
    storage: Vec<DrawPrimitive>,
    replay: &mut Option<&mut cranpose_ui_graphics::CommandReplayState>,
    command_id: Option<crate::graph::DrawCommandId>,
) -> (
    Vec<DrawPrimitive>,
    CommandRecording,
    Option<cranpose_ui_graphics::CommandReplayFrame>,
) {
    // `markers` is the recording scope's own count of `Content` markers,
    // maintained while the command records (`draw_content()` calls and pushed
    // batches both keep it current), so it is authoritative: zero means the
    // vector holds no marker and passes through untouched. On a watch-class
    // core, re-streaming a fresh multi-thousand-primitive recording just to
    // learn "no markers" is measurable frame time.
    let filter_content = |primitives: Vec<DrawPrimitive>, markers: u32| {
        if markers == 0 {
            return primitives;
        }
        // `filter(...).collect()` reports a zero lower-bound size hint, so it
        // grows the output through the whole doubling schedule; for an
        // animated scene these vectors hold thousands of primitives and are
        // rebuilt every frame. `Content` markers are rare, so the input
        // length is the right capacity.
        let mut out = Vec::with_capacity(primitives.len());
        out.extend(
            primitives
                .into_iter()
                .filter(|primitive| !matches!(primitive, DrawPrimitive::Content)),
        );
        out
    };

    let split_with_content = |primitives: Vec<DrawPrimitive>, placement, markers: u32| {
        let last_content_idx = if markers == 0 {
            None
        } else {
            primitives
                .iter()
                .rposition(|primitive| matches!(primitive, DrawPrimitive::Content))
        };
        let Some(last_content_idx) = last_content_idx else {
            return if matches!(placement, DrawPlacement::Overlay) {
                filter_content(primitives, markers)
            } else {
                Vec::new()
            };
        };

        let mut out = Vec::with_capacity(primitives.len());
        out.extend(
            primitives
                .into_iter()
                .enumerate()
                .filter_map(|(index, primitive)| {
                    if matches!(primitive, DrawPrimitive::Content) {
                        return None;
                    }
                    let is_before = index < last_content_idx;
                    match placement {
                        DrawPlacement::Behind if is_before => Some(primitive),
                        DrawPlacement::Overlay if !is_before => Some(primitive),
                        _ => None,
                    }
                }),
        );
        out
    };

    // The command records into a scope this consumer owns, so the marker
    // count travels with the recording instead of through a side channel.
    fn record_into(
        func: &DrawCommandFn,
        size: Size,
        recording: CommandRecording,
        storage: Vec<DrawPrimitive>,
        replay: &mut Option<&mut cranpose_ui_graphics::CommandReplayState>,
        command: Option<crate::graph::DrawCommandId>,
    ) -> (
        FinishedRecording,
        Option<cranpose_ui_graphics::CommandReplayFrame>,
    ) {
        let mut scope = cranpose_ui::command_draw_scope_retained(size, recording, storage);
        func(&mut scope);
        let Some(state) = replay.as_mut() else {
            return (scope.finish(), None);
        };
        let outcome =
            state.advance_pooled(scope.recorded(), crate::scene_builder::verify_executor());
        // Span counts are taken before `finish_replay` consumes the outcome
        // (recolor patch lists move into the frame instead of being cloned
        // every frame).
        let diag = if cranpose_core::env_flag!("CRANPOSE_COMMAND_REPLAY_DIAG") {
            if let cranpose_ui_graphics::ReplayOutcome::Spans(spans) = &outcome {
                let (mut retained, mut dynamic) = (0usize, 0usize);
                for span in spans {
                    match span {
                        cranpose_ui_graphics::ReplaySpan::Retained { .. } => retained += 1,
                        cranpose_ui_graphics::ReplaySpan::Dynamic {
                            tape_start,
                            tape_end,
                        } => dynamic += tape_end - tape_start,
                    }
                }
                Some((
                    scope.recorded().len(),
                    retained,
                    dynamic,
                    state.segments().len(),
                    state.stats(),
                ))
            } else {
                None
            }
        } else {
            None
        };
        let center = state.center();
        // Only spans whose retained buffer the renderer has confirmed may
        // skip materialization: everything else must exist as primitives
        // for this frame's ordinary path. A marker-bearing recording drops
        // its frame after the split, so it must materialize whole.
        let markers = scope.content_marker_count();
        let mut bypass = |slot: u32| {
            markers == 0
                && command.is_some_and(|id| crate::scene_builder::retained_slot_confirmed(id, slot))
        };
        let (finished, frame) = scope.finish_replay(center, outcome, &mut bypass);
        if let Some((records, retained, dynamic, segments, (deaths, splits))) = diag {
            log::warn!(
                "[command-replay] {} records: {} retained spans, {} dynamic records, \
                 {} materialized; {} segments alive, lifetime deaths {} splits {}",
                records,
                retained,
                dynamic,
                finished.primitives.len(),
                segments,
                deaths,
                splits,
            );
        }
        (finished, frame)
    }
    match (placement, command) {
        (DrawPlacement::Behind, DrawCommand::Behind(func)) => {
            let (finished, frame) = record_into(func, size, recording, storage, replay, command_id);
            let frame = (finished.content_markers == 0).then_some(frame).flatten();
            (
                filter_content(finished.primitives, finished.content_markers),
                finished.recording,
                frame,
            )
        }
        (DrawPlacement::Overlay, DrawCommand::Overlay(func)) => {
            let (finished, frame) = record_into(func, size, recording, storage, replay, command_id);
            let frame = (finished.content_markers == 0).then_some(frame).flatten();
            (
                filter_content(finished.primitives, finished.content_markers),
                finished.recording,
                frame,
            )
        }
        (_, DrawCommand::WithContent(func)) => {
            // Content splitting reindexes the vector; the frame's ranges
            // would not survive it. No id means no bypass either.
            let (finished, _) = record_into(func, size, recording, storage, replay, None);
            (
                split_with_content(finished.primitives, placement, finished.content_markers),
                finished.recording,
                None,
            )
        }
        _ => (Vec::new(), recording, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, Rect};

    fn recorded_command(record: impl Fn(&mut dyn DrawScope) + 'static) -> DrawCommandFn {
        Rc::new(move |scope: &mut DrawScopeDefault| record(scope))
    }

    fn rect_at(x: f32) -> Rect {
        Rect {
            x,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    fn rect_xs(primitives: &[DrawPrimitive]) -> Vec<f32> {
        primitives
            .iter()
            .map(|primitive| match primitive {
                DrawPrimitive::Rect { rect, .. } => rect.x,
                other => panic!("unexpected primitive {other:?}"),
            })
            .collect()
    }

    #[test]
    fn marker_free_recording_passes_through() {
        let command = DrawCommand::Behind(recorded_command(|scope| {
            scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
            scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
        }));
        let out = primitives_for_placement(&command, DrawPlacement::Behind, Size::new(10.0, 10.0));
        assert_eq!(rect_xs(&out), [1.0, 2.0]);
    }

    #[test]
    fn recorded_markers_still_split_content_placements() {
        let with_content = recorded_command(|scope| {
            scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
            scope.draw_content();
            scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
        });
        let command = DrawCommand::WithContent(with_content);
        let size = Size::new(10.0, 10.0);
        let behind = primitives_for_placement(&command, DrawPlacement::Behind, size);
        assert_eq!(rect_xs(&behind), [1.0]);
        let overlay = primitives_for_placement(&command, DrawPlacement::Overlay, size);
        assert_eq!(rect_xs(&overlay), [2.0]);
    }

    /// Recording into a previously used buffer must be indistinguishable
    /// from recording into a fresh one — for every placement, including the
    /// marker-splitting paths.
    #[test]
    fn reused_storage_records_identically_to_fresh() {
        let command = DrawCommand::WithContent(recorded_command(|scope| {
            scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
            scope.draw_content();
            scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
        }));
        let size = Size::new(10.0, 10.0);
        for placement in [DrawPlacement::Behind, DrawPlacement::Overlay] {
            let fresh = primitives_for_placement(&command, placement, size);
            // Junk in the reused buffer must not leak into the recording.
            let dirty = vec![DrawPrimitive::Content; 8];
            let reused = primitives_for_placement_reusing(&command, placement, size, dirty);
            assert_eq!(fresh, reused);
        }
    }

    #[test]
    fn pushed_batches_keep_marker_count_authoritative() {
        // A pre-built vector enters through `push_recorded`, which counts the
        // `Content` marker it carries; the filter path must then strip it.
        let command = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
            scope.push_recorded(vec![
                DrawPrimitive::Rect {
                    rect: rect_at(1.0),
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                DrawPrimitive::Content,
                DrawPrimitive::Rect {
                    rect: rect_at(2.0),
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
            ]);
        }));
        let out = primitives_for_placement(&command, DrawPlacement::Behind, Size::new(10.0, 10.0));
        assert_eq!(rect_xs(&out), [1.0, 2.0]);
    }
}
