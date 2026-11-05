use std::{borrow::Cow, ops::Range, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui::{DrawCommand, Point, Rect, Size};
use cranpose_ui_graphics::{
    CommandRecording, CompositingStrategy, DrawGroup, DrawPlacement, GraphicsLayer, RecordLane,
};

use super::{LayerHead, assign_layer};
use crate::{
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, PrimitivePhase, RenderNode,
    },
    style_shared::recording_for_placement_reusing,
};

pub(crate) struct RecorderSlot {
    generation: u64,
    handles: [Option<Rc<CommandRecording>>; 3],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RecorderKey {
    node_id: NodeId,
    command_index: u32,
}

impl From<DrawCommandId> for RecorderKey {
    fn from(id: DrawCommandId) -> Self {
        Self {
            node_id: id.node_id,
            command_index: id.command_index,
        }
    }
}

#[doc(hidden)]
pub fn clear_command_recordings_for_tests() {
    crate::thread_state::with(|state| state.command_recordings.borrow_mut().clear());
}

pub(super) fn bump_recording_generation() {
    crate::thread_state::with(|state| {
        let generation = state.recording_generation.get().wrapping_add(1);
        state.recording_generation.set(generation);
        if generation.is_multiple_of(512) {
            state
                .command_recordings
                .borrow_mut()
                .retain(|_, slot| generation.wrapping_sub(slot.generation) <= 64);
        }
    });
}

/// A recording of the command `id` that no reader holds any more, to draw
/// the command again in place: first one whose shape columns no reader
/// shares either, then the oldest of three, whose recording takes new
/// columns with the room the old ones had. A new one when none is free.
pub(super) fn acquire_storage(id: DrawCommandId) -> Rc<CommandRecording> {
    crate::thread_state::with(|state| {
        let mut map = state.command_recordings.borrow_mut();
        let Some(slot) = map.get_mut(&RecorderKey::from(id)) else {
            return Rc::default();
        };
        let free = |handle: &Option<Rc<CommandRecording>>| {
            handle.as_ref().is_some_and(|recording| {
                Rc::strong_count(recording) == 1 && Rc::weak_count(recording) == 0
            })
        };
        let unshared = slot.handles.iter().position(|handle| {
            free(handle)
                && handle
                    .as_ref()
                    .is_some_and(|recording| !recording.shares_shapes())
        });
        let at = unshared.or_else(|| {
            (slot.handles.iter().all(Option::is_some) && free(&slot.handles[2])).then_some(2)
        });
        at.and_then(|at| slot.handles[at].take())
            .unwrap_or_default()
    })
}

/// Keeps `recording`, the command `id` drew this frame, as the newest of
/// the command's three, and hands it back for the frame's scene.
pub(super) fn publish_recording(
    id: DrawCommandId,
    recording: Rc<CommandRecording>,
) -> Rc<CommandRecording> {
    crate::thread_state::with(|state| {
        let mut map = state.command_recordings.borrow_mut();
        let generation = state.recording_generation.get();
        let slot = map
            .entry(RecorderKey::from(id))
            .or_insert_with(|| RecorderSlot {
                generation,
                handles: [None, None, None],
            });
        slot.generation = generation;
        slot.handles[2] = slot.handles[1].take();
        slot.handles[1] = slot.handles[0].take();
        slot.handles[0] = Some(Rc::clone(&recording));
        recording
    })
}

pub(super) enum DrawPass<'a> {
    Record(DrawPlacement),
    OverlayFrom(std::slice::Iter<'a, RenderNode>),
    OverlayFromOutput(usize),
}

impl DrawPass<'_> {
    fn placement(&self) -> DrawPlacement {
        match self {
            Self::Record(placement) => *placement,
            Self::OverlayFrom(_) | Self::OverlayFromOutput(_) => DrawPlacement::Overlay,
        }
    }

    fn take_recording(
        &mut self,
        command: &DrawCommand,
        output: &[RenderNode],
    ) -> Option<Rc<CommandRecording>> {
        if matches!(command, DrawCommand::Overlay(_)) {
            return None;
        }
        let node = match self {
            Self::Record(_) => return None,
            Self::OverlayFrom(nodes) => nodes.next().expect("each behind command has a draw run"),
            Self::OverlayFromOutput(index) => {
                let node = &output[*index];
                *index += 1;
                node
            }
        };
        // A command whose draws a group places is a layer that starts
        // with the run before its first group.
        let run = match node {
            RenderNode::DrawRun(run) => run,
            RenderNode::Layer(layer) => match layer.children.first() {
                Some(RenderNode::DrawRun(run)) => run,
                _ => return None,
            },
            RenderNode::Primitive(_) | RenderNode::Inline(_) => return None,
        };
        matches!(command, DrawCommand::WithContent(_)).then(|| Rc::clone(&run.recording))
    }
}

/// Where a node's draw commands draw: the scope size, and what the layers
/// of their groups take from the node: its motion context and the paint of
/// its graphics layer, which the node's own draws take.
#[derive(Clone, Copy)]
pub(super) struct DrawTarget<'a> {
    pub(super) size: Size,
    pub(super) motion_context_animated: bool,
    pub(super) layer: Option<&'a GraphicsLayer>,
}

/// Writes the part of one draw command's recording that a draw pass draws,
/// turning each group its draws open into a layer that the group places.
pub(super) struct GroupedDraws<'a> {
    id: DrawCommandId,
    phase: PrimitivePhase,
    recording: &'a Rc<CommandRecording>,
    motion_context_animated: bool,
    /// The paint of the node's graphics layer that its own draws take, when
    /// it paints them: the group layers take it as well, as the group's
    /// draws are the node's own, where the node's child layers do not.
    paint: Option<GraphicsLayer>,
}

impl<'a> GroupedDraws<'a> {
    fn new(
        id: DrawCommandId,
        phase: PrimitivePhase,
        recording: &'a Rc<CommandRecording>,
        draws: DrawTarget<'_>,
    ) -> Self {
        Self {
            id,
            phase,
            recording,
            motion_context_animated: draws.motion_context_animated,
            paint: draws
                .layer
                .map(crate::layer_composition::local_content_layer_for)
                .filter(|paint| paint.alpha != 1.0 || paint.color_filter.is_some())
                .map(Cow::into_owned),
        }
    }

    /// One layer that holds the draws of `segments` in the parent's own
    /// draws, the groups among them as layers. It starts with the run
    /// before the first group, even an empty one, where the overlay pass of
    /// a with-content command finds the recording.
    fn wrapper(&self, segments: Range<u32>, size: Size) -> RenderNode {
        let first_group = self
            .recording
            .segments_in(&segments)
            .position(|segment| segment.lane == RecordLane::GroupStart)
            .map_or(segments.end, |offset| segments.start + offset as u32);
        let mut wrapper = crate::layer_recycling::layer_box();
        wrapper.children.push(self.run(segments.start..first_group));
        self.write(&mut wrapper.children, first_group, segments.end);
        assign_layer(
            &mut wrapper,
            self.head(&DrawGroup::plain(Rect::from_size(size))),
        );
        RenderNode::Layer(wrapper)
    }

    /// Writes the segments from `position` into `list` up to the end of the
    /// group open at `position` or up to `end`, and returns the position
    /// after what it wrote.
    fn write(&self, list: &mut Vec<RenderNode>, mut position: u32, end: u32) -> u32 {
        let segments = self.recording.segments();
        let mut run_start = position;
        while position < end {
            let segment = segments[position as usize];
            match segment.lane {
                RecordLane::GroupStart => {
                    self.push_run(list, run_start..position);
                    let mut layer = crate::layer_recycling::layer_box();
                    position = self.write(&mut layer.children, position + 1, end);
                    let group = self.recording.groups()[segment.start as usize];
                    assign_layer(&mut layer, self.head(&group));
                    list.push(RenderNode::Layer(layer));
                    run_start = position;
                }
                RecordLane::GroupEnd => {
                    self.push_run(list, run_start..position);
                    return position + 1;
                }
                RecordLane::Shapes | RecordLane::Others | RecordLane::Content => position += 1,
            }
        }
        self.push_run(list, run_start..end);
        end
    }

    fn run(&self, segments: Range<u32>) -> RenderNode {
        RenderNode::DrawRun(DrawRunNode::for_command_shared(
            self.phase,
            Some(self.id),
            Rc::clone(self.recording),
            segments,
        ))
    }

    fn push_run(&self, list: &mut Vec<RenderNode>, segments: Range<u32>) {
        if !segments.is_empty() {
            list.push(self.run(segments));
        }
    }

    /// The graphics layer of a group's layer: the group's scale and turn,
    /// and the node's paint. An isolated group takes the alpha as it
    /// composites, so the mask it ends with keeps its own.
    fn group_layer(&self, group: &DrawGroup) -> Option<GraphicsLayer> {
        if self.paint.is_none() && !group.scales_or_turns() {
            return None;
        }
        let mut layer = group.layer();
        if let Some(paint) = &self.paint {
            layer.alpha = paint.alpha;
            layer.color_filter.clone_from(&paint.color_filter);
            if !group.isolated {
                layer.compositing_strategy = CompositingStrategy::ModulateAlpha;
            }
        }
        Some(layer)
    }

    fn head(&self, group: &DrawGroup) -> LayerHead {
        let bounds = group.bounds;
        // The group's draws sit in its coordinates, whose origin a clip's
        // bounds may leave, as a clip bounds a node's layer away from it.
        let node_bounds = (bounds.x != 0.0 || bounds.y != 0.0).then(|| Rect {
            x: 0.0,
            y: 0.0,
            width: (bounds.x + bounds.width).max(0.0),
            height: (bounds.y + bounds.height).max(0.0),
        });
        LayerHead {
            node_id: None,
            wraps: None,
            local_bounds: bounds,
            node_bounds,
            transform_to_parent: group.transform_to_parent(),
            content_offset: Point::default(),
            motion_context_animated: self.motion_context_animated,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            origin_in_parent: Point::default(),
            graphics_layer: self.group_layer(group),
            clip_to_bounds: group.clip,
            hit_test: None,
            has_origin_sinks: false,
            isolation: IsolationReasons {
                explicit_offscreen: group.isolated,
                group_opacity: group.isolated
                    && self.paint.as_ref().is_some_and(|paint| paint.alpha < 1.0),
                ..IsolationReasons::default()
            },
            cache_policy: CachePolicy::None,
            draw_phase: Some(self.phase),
        }
    }
}

pub(super) fn layer_node_capacity(
    commands: &[DrawCommand],
    children: usize,
    has_text: bool,
) -> usize {
    children
        + usize::from(has_text)
        + commands.len()
        + commands
            .iter()
            .filter(|command| matches!(command, DrawCommand::WithContent(_)))
            .count()
}

pub(super) fn draw_nodes(
    node_id: NodeId,
    commands: &[DrawCommand],
    first_command_index: usize,
    placement: DrawPlacement,
    draws: DrawTarget,
    phase: PrimitivePhase,
) -> Vec<RenderNode> {
    let mut nodes = crate::layer_recycling::child_list(commands.len());
    append_draw_nodes(
        &mut nodes,
        node_id,
        commands,
        first_command_index,
        DrawPass::Record(placement),
        draws,
        phase,
    );
    nodes
}

pub(super) fn append_draw_nodes(
    nodes: &mut Vec<RenderNode>,
    node_id: NodeId,
    commands: &[DrawCommand],
    first_command_index: usize,
    mut pass: DrawPass<'_>,
    draws: DrawTarget,
    phase: PrimitivePhase,
) {
    let size = draws.size;
    let placement = pass.placement();
    for (command_index, command) in commands.iter().enumerate() {
        let id = DrawCommandId {
            node_id,
            command_index: (first_command_index + command_index) as u32,
            placement,
        };
        let (shared, segments) = if let Some(shared) = pass.take_recording(command, nodes) {
            let segments = shared.content_split(DrawPlacement::Overlay);
            (shared, segments)
        } else {
            let Some((recording, segments)) =
                recording_for_placement_reusing(command, placement, size, || acquire_storage(id))
            else {
                continue;
            };
            (publish_recording(id, recording), segments)
        };
        let grouped = shared.summary().has_groups
            && shared
                .segments_in(&segments)
                .any(|segment| segment.lane == RecordLane::GroupStart);
        nodes.push(if grouped {
            GroupedDraws::new(id, phase, &shared, draws).wrapper(segments, size)
        } else {
            RenderNode::DrawRun(DrawRunNode::for_command_shared(
                phase,
                Some(id),
                shared,
                segments,
            ))
        });
    }
}

#[doc(hidden)]
pub fn draw_command_nodes_for_tests(
    node_id: NodeId,
    commands: &[DrawCommand],
    placement: DrawPlacement,
    size: Size,
    phase: PrimitivePhase,
) -> Vec<RenderNode> {
    bump_recording_generation();
    let draws = DrawTarget {
        size,
        motion_context_animated: false,
        layer: None,
    };
    draw_nodes(node_id, commands, 0, placement, draws, phase)
}
