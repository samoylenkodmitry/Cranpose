use std::{borrow::Cow, hash::Hasher, ops::Range, rc::Rc};

use cranpose_core::{NodeId, RecordId};
use cranpose_ui::{BackgroundDraw, Color, DrawCommand, ModifierNodeSlices, Point, Rect, Size};
use cranpose_ui_graphics::{
    CommandRecording, CompositingStrategy, CornerRadii, DrawGroup, DrawPlacement, FxHasher,
    GraphicsLayer, RecordLane,
};

use super::{
    HeadKind, LayerHead, assign_layer,
    text_node::{TextNodeParts, refresh_text_node},
};
use crate::{
    graph::{CachePolicy, DrawCommandId, DrawRunNode, IsolationReasons, PrimitivePhase},
    scene_store::{Item, ItemKind, ItemRef, RecordKind, RecordSeed, SceneStore},
    style_shared::{draws_in, record_in_place, recording_for_placement_reusing},
};

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
            ItemRef::Draw(_) | ItemRef::Text(_) | ItemRef::Leaf(_) | ItemRef::Shape(_) => {
                return None;
            }
        };
        matches!(command, DrawCommand::WithContent(_)).then(|| Rc::clone(&run.recording))
    }
}

#[derive(Clone, Copy)]
pub(super) struct DrawTarget<'a> {
    pub(super) size: Size,
    pub(super) motion_context_animated: bool,
    pub(super) paint: Option<GroupPaint<'a>>,
}

// Where the layers of a node's draw groups take their paint from: a graphics
// layer the builder holds, or the one a record holds in the store.
#[derive(Clone, Copy)]
pub(super) enum GroupPaint<'a> {
    Layer(&'a GraphicsLayer),
    Record(RecordId),
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
        store: &SceneStore,
        id: DrawCommandId,
        phase: PrimitivePhase,
        recording: &'a Rc<CommandRecording>,
        draws: DrawTarget<'_>,
    ) -> Self {
        let layer = match draws.paint {
            Some(GroupPaint::Layer(layer)) => Some(layer),
            Some(GroupPaint::Record(record)) => store.graphics_layer(record),
            None => None,
        };
        Self {
            id,
            phase,
            recording,
            motion_context_animated: draws.motion_context_animated,
            paint: layer
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
        let mut list = store.lists.take();
        list.push(store.add_run(self.run(segments.start..first_group)));
        self.write(store, wrapper, &mut list, first_group, segments.end);
        store.set_items(wrapper, &list);
        store.lists.give(list);
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
                    let mut nested = store.lists.take();
                    position = self.write(store, record, &mut nested, position + 1, end);
                    store.set_items(record, &nested);
                    store.lists.give(nested);
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
    let mut items = store.lists.take();
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
            // A background takes shapes in the store's chunks, behind the
            // node's content alone.
            if let DrawCommand::Background(background) = command {
                if placement == DrawPlacement::Behind {
                    items.push(add_background(store, background, size));
                }
                continue;
            }
            // The overlay pass skips a command that draws behind alone
            // before it looks for the command's recording.
            if !draws_in(command, placement) {
                continue;
            }
            let Some(recorded) = record_command(store, command, id, size) else {
                continue;
            };
            recorded
        };
        items.push(if opens_group(&shared, &segments) {
            grouped_item(store, parent, (id, phase), &shared, draws, segments)
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

// Whether `segments` of `recording` open a draw group, which a layer of its
// own places.
fn opens_group(recording: &CommandRecording, segments: &Range<u32>) -> bool {
    recording.summary().has_groups
        && recording
            .segments_in(segments)
            .any(|segment| segment.lane == RecordLane::GroupStart)
}

// One step of a node's draws as a redraw in place writes them over the
// node's items, in draw order: a background, a command that records a run,
// or the node's text.
enum DrawStep<'a> {
    Background(&'a BackgroundDraw),
    Run(&'a DrawCommand, DrawCommandId),
    Text(&'a ModifierNodeSlices),
}

// Hands `visit` each step of `commands` in draw order: the draws behind the
// content, then `text`, then the draws over it. False when a command draws
// around the content, or when `visit` returns false.
fn walk_steps<'a>(
    commands: NodeCommands<'a>,
    text: Option<&'a ModifierNodeSlices>,
    mut visit: impl FnMut(DrawStep<'a>) -> bool,
) -> bool {
    for placement in [DrawPlacement::Behind, DrawPlacement::Overlay] {
        if placement == DrawPlacement::Overlay
            && let Some(text) = text
            && !visit(DrawStep::Text(text))
        {
            return false;
        }
        for (offset, command) in commands.commands.iter().enumerate() {
            let step = match command {
                DrawCommand::WithContent(_) => return false,
                DrawCommand::Background(_) if placement == DrawPlacement::Overlay => continue,
                DrawCommand::Background(background) => DrawStep::Background(background),
                _ if !draws_in(command, placement) => continue,
                _ => DrawStep::Run(
                    command,
                    DrawCommandId {
                        node_id: commands.node_id,
                        command_index: (commands.first_index + offset) as u32,
                        placement,
                    },
                ),
            };
            if !visit(step) {
                return false;
            }
        }
    }
    true
}

// Draws `commands`, a node's draw commands at size `size`, over `own`, the
// items of its record that drew them before, in draw order without its
// children, when they come out as the same kinds in the same order: a
// background whose shape and color stay keeps its shapes, a run records
// again into its own recording, and the text of `text`, the node's slices
// when it draws text, writes into the text it drew. False when other items come out,
// a command draws around the content, a run drew another command or a
// recording opens a draw group; the caller then writes the node's draws
// anew over what this wrote, and takes the recordings this made, which it
// parks, so no draw callback runs twice.
pub(super) fn redraw_in_place(
    store: &mut SceneStore,
    own: impl Iterator<Item = Item> + Clone,
    commands: NodeCommands<'_>,
    text: Option<&ModifierNodeSlices>,
    size: Size,
) -> bool {
    let node_id = commands.node_id;
    let mut items = own.clone();
    let mut drawn = 0;
    let all = walk_steps(commands, text, |step| {
        let again = items.next().is_some_and(|item| match step {
            DrawStep::Background(background) => {
                draws_background_again(store, item, background, size)
            }
            DrawStep::Run(command, id) => draws_run_again(store, item, (command, id), size),
            DrawStep::Text(slices) => draws_text_again(store, item, (node_id, slices), size),
        });
        drawn += usize::from(again);
        again
    }) && items.next().is_none();
    if !all {
        park_runs(store, own.take(drawn));
    }
    all
}

// Parks the recordings the runs among `drawn` made in this update, for the
// fallback that writes their node's draws anew.
#[cold]
#[inline(never)]
fn park_runs(store: &mut SceneStore, drawn: impl Iterator<Item = Item>) {
    for item in drawn.filter(|item| item.kind() == ItemKind::Run) {
        let run = store.run(item);
        if let Some(id) = run.command {
            let recorded = (Rc::clone(&run.recording), run.segments.clone());
            store.recordings.park(id, recorded);
        }
    }
}

// Writes the text that `slices` of the node `node_id` draw at `size` into
// the text item `item`. False when `item` is no text item or the node draws
// no text now. Out of line: most nodes a redraw takes draw no text.
#[inline(never)]
fn draws_text_again(
    store: &mut SceneStore,
    item: Item,
    (node_id, slices): (NodeId, &ModifierNodeSlices),
    size: Size,
) -> bool {
    item.kind() == ItemKind::Text
        && refresh_text_node(
            store.text_mut(item),
            &TextNodeParts::of(node_id, slices, size),
        )
}

// Draws `background` at `size` into the shape item `item`, which keeps its
// shapes while their print stays. False when `item` is no shape item.
fn draws_background_again(
    store: &mut SceneStore,
    item: Item,
    background: &BackgroundDraw,
    size: Size,
) -> bool {
    if item.kind() != ItemKind::Shape {
        return false;
    }
    let shape = background.shape_in(size);
    let print = background_print(shape, background.color());
    if store.shapes_print(item) != print {
        store.redraw_shapes(item, print, |recorder| background.record(shape, recorder));
    }
    true
}

// Records `command`, the command `id`, again into the run `item` that drew
// it before: into the run's own recording when no reader holds it, else
// into storage the store keeps for the command. False when `item` is no
// run of the command, or when the new recording opens a draw group, which
// it then parks for the fallback.
#[inline(never)]
fn draws_run_again(
    store: &mut SceneStore,
    item: Item,
    (command, id): (&DrawCommand, DrawCommandId),
    size: Size,
) -> bool {
    if item.kind() != ItemKind::Run || store.run(item).command != Some(id) {
        return false;
    }
    let in_place = Rc::get_mut(&mut store.run_mut(item).recording)
        .map(|recording| record_in_place(command, id.placement, size, recording));
    let segments = match in_place {
        Some(segments) => segments,
        None => record_command(store, command, id, size).map(|(recording, segments)| {
            store.swap_run_recording(item, recording);
            segments
        }),
    };
    let Some(segments) = segments else {
        return false;
    };
    let run = store.run_mut(item);
    if opens_group(&run.recording, &segments) {
        let recording = Rc::clone(&run.recording);
        store.recordings.park(id, (recording, segments));
        return false;
    }
    run.summary = run.recording.summary_in(&segments);
    run.segments = segments;
    true
}

// The three paths below run out of line, so the loop over a node's
// commands stays small: the ESP32 boards run it from the instruction cache.

// The item of a background's shapes, recorded into the store's chunks
// unless the item the node drew them in before gives them back.
#[inline(never)]
fn add_background(store: &mut SceneStore, background: &BackgroundDraw, size: Size) -> Item {
    let shape = background.shape_in(size);
    store.add_shapes(background_print(shape, background.color()), |recorder| {
        background.record(shape, recorder);
    })
}

// A hash of a background's shape: its rect, its corners when they are
// round, and its color. Two numbers take one word: a fold of a word costs a
// 32-bit target several multiplies.
fn background_print(
    (rect, radii): (Rect, Option<CornerRadii>),
    Color(red, green, blue, alpha): Color,
) -> u64 {
    let pair = |a: f32, b: f32| u64::from(a.to_bits()) << 32 | u64::from(b.to_bits());
    let mut print = FxHasher::default();
    for word in [
        pair(rect.x, rect.y),
        pair(rect.width, rect.height),
        pair(red, green),
        pair(blue, alpha),
    ] {
        print.write_u64(word);
    }
    if let Some(radii) = radii {
        print.write_u64(pair(radii.top_left, radii.top_right));
        print.write_u64(pair(radii.bottom_right, radii.bottom_left));
    }
    print.finish()
}

// Records `command`, the command `id`, anew into storage its kept
// recordings lend, and notes the recording.
#[inline(never)]
fn record_command(
    store: &mut SceneStore,
    command: &DrawCommand,
    id: DrawCommandId,
    size: Size,
) -> Option<(Rc<CommandRecording>, Range<u32>)> {
    if let Some(parked) = store.recordings.take_parked(id) {
        return Some(parked);
    }
    store.recordings.record(id, |storage| {
        recording_for_placement_reusing(command, id.placement, size, storage)
    })
}

// The layer a recording's draw groups take, in a wrapper under `parent`.
#[cold]
#[inline(never)]
fn grouped_item(
    store: &mut SceneStore,
    parent: RecordId,
    (id, phase): (DrawCommandId, PrimitivePhase),
    recording: &Rc<CommandRecording>,
    draws: DrawTarget<'_>,
    segments: Range<u32>,
) -> Item {
    GroupedDraws::new(store, id, phase, recording, draws)
        .wrapper(store, parent, segments, draws.size)
}

// Lowers `commands`, the draw commands of node `node_id` of size `size`,
// into items of `record` that paint in `phase`, as the scene builder lowers
// a node's draws: runs, and layers for the draws a `DrawScope` groups.
pub(crate) fn draw_command_items(
    store: &mut SceneStore,
    record: RecordId,
    (node_id, size): (NodeId, Size),
    commands: &[DrawCommand],
    placement: DrawPlacement,
    phase: PrimitivePhase,
) -> Vec<Item> {
    let draws = DrawTarget {
        size,
        motion_context_animated: false,
        paint: None,
    };
    let commands = NodeCommands {
        node_id,
        commands,
        first_index: 0,
    };
    draw_nodes(store, record, commands, placement, draws, phase)
}
