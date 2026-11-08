use std::{borrow::Cow, ops::Range, rc::Rc};

use cranpose_core::{NodeId, RecordId};
use cranpose_ui::{DrawCommand, Point, Rect, Size};
use cranpose_ui_graphics::{
    CommandRecording, CompositingStrategy, DrawGroup, DrawPlacement, GraphicsLayer, RecordLane,
};

use super::{HeadKind, LayerHead, assign_layer, item_lists};
use crate::{
    graph::{
        CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, PrimitivePhase, RenderNode,
    },
    scene_store::{Item, ItemRef, RecordKind, RecordSeed, SceneStore},
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
    OverlayFrom(std::slice::Iter<'a, Item>),
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
        store: &SceneStore,
        command: &DrawCommand,
        output: &[Item],
    ) -> Option<Rc<CommandRecording>> {
        if matches!(command, DrawCommand::Overlay(_)) {
            return None;
        }
        let item = match self {
            Self::Record(_) => return None,
            Self::OverlayFrom(items) => *items.next().expect("each behind command has a draw run"),
            Self::OverlayFromOutput(index) => {
                let item = output[*index];
                *index += 1;
                item
            }
        };
        // A command whose draws a group places is a layer that starts
        // with the run before its first group.
        let run = match ItemRef::of(store, item) {
            ItemRef::Run(run) => run,
            ItemRef::Layer(group) => match group.items().next() {
                Some(ItemRef::Run(run)) => run,
                _ => return None,
            },
            ItemRef::Draw(_) | ItemRef::Text(_) | ItemRef::Leaf(_) => return None,
        };
        matches!(command, DrawCommand::WithContent(_)).then(|| Rc::clone(&run.recording))
    }
}

#[derive(Clone, Copy)]
pub(super) struct DrawTarget<'a> {
    pub(super) size: Size,
    pub(super) motion_context_animated: bool,
    pub(super) layer: Option<&'a GraphicsLayer>,
}

pub(super) struct GroupedDraws<'a> {
    id: DrawCommandId,
    phase: PrimitivePhase,
    recording: &'a Rc<CommandRecording>,
    motion_context_animated: bool,
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

    fn wrapper(
        &self,
        store: &mut SceneStore,
        parent: RecordId,
        segments: Range<u32>,
        size: Size,
    ) -> Item {
        let first_group = self
            .recording
            .segments_in(&segments)
            .position(|segment| segment.lane == RecordLane::GroupStart)
            .map_or(segments.end, |offset| segments.start + offset as u32);
        let wrapper = self.group_record(store, parent);
        let mut list = item_lists::take();
        list.push(store.add_run(self.run(segments.start..first_group)));
        self.write(store, wrapper, &mut list, first_group, segments.end);
        store.set_items(wrapper, &list);
        item_lists::give(list);
        assign_layer(
            store,
            wrapper,
            self.head(&DrawGroup::plain(Rect::from_size(size))),
        );
        Item::child(wrapper)
    }

    fn group_record(&self, store: &mut SceneStore, parent: RecordId) -> RecordId {
        let record = store.insert(RecordSeed {
            kind: RecordKind::DrawGroup,
            node: Some(self.id.node_id),
            parent: Some(parent),
        });
        store.set_content_offset(record, Point::default());
        record
    }

    fn write(
        &self,
        store: &mut SceneStore,
        parent: RecordId,
        list: &mut Vec<Item>,
        mut position: u32,
        end: u32,
    ) -> u32 {
        let segments = self.recording.segments();
        let mut run_start = position;
        while position < end {
            let segment = segments[position as usize];
            match segment.lane {
                RecordLane::GroupStart => {
                    self.push_run(store, list, run_start..position);
                    let record = self.group_record(store, parent);
                    let mut nested = item_lists::take();
                    position = self.write(store, record, &mut nested, position + 1, end);
                    store.set_items(record, &nested);
                    item_lists::give(nested);
                    let group = self.recording.groups()[segment.start as usize];
                    assign_layer(store, record, self.head(&group));
                    list.push(Item::child(record));
                    run_start = position;
                }
                RecordLane::GroupEnd => {
                    self.push_run(store, list, run_start..position);
                    return position + 1;
                }
                RecordLane::Shapes | RecordLane::Others | RecordLane::Content => position += 1,
            }
        }
        self.push_run(store, list, run_start..end);
        end
    }

    fn run(&self, segments: Range<u32>) -> DrawRunNode {
        DrawRunNode::for_command_shared(
            self.phase,
            Some(self.id),
            Rc::clone(self.recording),
            segments,
        )
    }

    fn push_run(&self, store: &mut SceneStore, list: &mut Vec<Item>, segments: Range<u32>) {
        if !segments.is_empty() {
            list.push(store.add_run(self.run(segments)));
        }
    }

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
        let transform_to_parent = group.transform_to_parent();
        LayerHead {
            kind: HeadKind::Group {
                node: self.id.node_id,
                phase: self.phase,
            },
            local_bounds: bounds,
            node_bounds,
            transform_to_parent,
            content_offset: Point::default(),
            motion_context_animated: self.motion_context_animated,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            origin_in_parent: transform_to_parent.as_translation().unwrap_or_default(),
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

#[derive(Clone, Copy)]
pub(super) struct NodeCommands<'a> {
    pub(super) node_id: NodeId,
    pub(super) commands: &'a [DrawCommand],
    pub(super) first_index: usize,
}

pub(super) fn draw_nodes(
    store: &mut SceneStore,
    parent: RecordId,
    commands: NodeCommands<'_>,
    placement: DrawPlacement,
    draws: DrawTarget,
    phase: PrimitivePhase,
) -> Vec<Item> {
    let mut items = item_lists::take();
    append_draw_nodes(
        store,
        parent,
        &mut items,
        commands,
        DrawPass::Record(placement),
        draws,
        phase,
    );
    items
}

pub(super) fn append_draw_nodes(
    store: &mut SceneStore,
    parent: RecordId,
    items: &mut Vec<Item>,
    commands: NodeCommands<'_>,
    mut pass: DrawPass<'_>,
    draws: DrawTarget,
    phase: PrimitivePhase,
) {
    let size = draws.size;
    let placement = pass.placement();
    for (command_index, command) in commands.commands.iter().enumerate() {
        let id = DrawCommandId {
            node_id: commands.node_id,
            command_index: (commands.first_index + command_index) as u32,
            placement,
        };
        let (shared, segments) = if let Some(shared) = pass.take_recording(store, command, items) {
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
        items.push(if grouped {
            GroupedDraws::new(id, phase, &shared, draws).wrapper(store, parent, segments, size)
        } else {
            store.add_run(DrawRunNode::for_command_shared(
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
    let mut store = SceneStore::new();
    store.begin_update();
    let root = store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(node_id),
        parent: None,
    });
    let commands = NodeCommands {
        node_id,
        commands,
        first_index: 0,
    };
    let items = draw_nodes(&mut store, root, commands, placement, draws, phase);
    store.set_items(root, &items);
    item_lists::give(items);
    store.finish_update();
    crate::scene_store::export(&store).children
}
