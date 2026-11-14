use std::{borrow::Cow, hash::Hasher, ops::Range, rc::Rc};

use cranpose_core::{NodeId, RecordId};
use cranpose_ui::{BackgroundDraw, Color, DrawCommand, ModifierNodeSlices, Point, Rect, Size};
use cranpose_ui_graphics::{
    CommandRecording, CompositingStrategy, CornerRadii, DrawGroup, DrawPlacement, FxHasher,
    GraphicsLayer, RecordLane,
};

use super::{
    HeadKind, LayerHead, assign_layer,
    text_node::{TextNodeParts, refresh_text_node, text_node_from_parts},
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

    // The recording the behind half of `command` made, for the overlay
    // half this pass places: `None` in a pass that records, and for a
    // command that draws on one side of the content alone.
    #[inline]
    fn take_recording(
        &mut self,
        store: &SceneStore,
        command: &DrawCommand,
        output: &[Item],
    ) -> Option<Rc<CommandRecording>> {
        let item = match self {
            Self::Record(_) => return None,
            _ if matches!(command, DrawCommand::Overlay(_)) => return None,
            Self::OverlayFrom(items) => *items.next().expect("each behind command has a draw run"),
            Self::OverlayFromOutput(index) => {
                let item = output[*index];
                *index += 1;
                item
            }
        };
        if !matches!(command, DrawCommand::WithContent(_)) {
            return None;
        }
        behind_recording(store, item)
    }
}

// The recording of `item`, which the behind half of a with-content command
// drew: a run, or the layer of a draw group, which starts with the run
// before its first group.
#[inline(never)]
fn behind_recording(store: &SceneStore, item: Item) -> Option<Rc<CommandRecording>> {
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
    Some(Rc::clone(&run.recording))
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

// The items a record drew of its own before an update writes its node's
// draws again, in draw order: those before the record's children, then
// those after them. A draw takes the old item in its place when that one can
// hold it, and a new item otherwise. The old items no draw took go when the
// write ends.
pub(super) struct OwnDraws<'a> {
    old: [&'a [Item]; 2],
    next: usize,
}

impl<'a> OwnDraws<'a> {
    // No old items: every draw takes a new item.
    pub(super) const fn none() -> Self {
        Self::of(&[], &[])
    }

    pub(super) const fn of(before: &'a [Item], after: &'a [Item]) -> Self {
        Self {
            old: [before, after],
            next: 0,
        }
    }

    fn at(&self, index: usize) -> Option<Item> {
        let [before, after] = self.old;
        match before.get(index) {
            Some(item) => Some(*item),
            None => after.get(index - before.len()).copied(),
        }
    }

    // The old item in the next draw's place, when it `fits` the draw.
    #[inline]
    fn take(
        &mut self,
        store: &SceneStore,
        fits: impl FnOnce(&SceneStore, Item) -> bool,
    ) -> Option<Item> {
        let item = self.at(self.next)?;
        if !fits(store, item) {
            return None;
        }
        self.next += 1;
        Some(item)
    }

    // Removes the old items no draw took, once the record holds its new
    // items.
    pub(super) fn finish(self, store: &mut SceneStore) {
        let mut index = self.next;
        while let Some(item) = self.at(index) {
            store.remove_item(item);
            index += 1;
        }
    }
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

    // The wrapper of `segments` under `parent`: `kept`, the wrapper the
    // command drew in before, or a new one.
    fn wrapper(
        &self,
        store: &mut SceneStore,
        parent: RecordId,
        segments: Range<u32>,
        size: Size,
        kept: Option<RecordId>,
    ) -> Item {
        let wrapper = match kept {
            Some(kept) if self.redraw_kept(store, kept, &segments) => kept,
            kept => {
                let first_group = self
                    .recording
                    .segments_in(&segments)
                    .position(|segment| segment.lane == RecordLane::GroupStart)
                    .map_or(segments.end, |offset| segments.start + offset as u32);
                let wrapper = kept.unwrap_or_else(|| self.group_record(store, parent));
                self.write_record(store, wrapper, |draws, store, list, own| {
                    // The wrapper starts with a run, which names its command.
                    list.push(draws.run_item(store, own, segments.start..first_group));
                    draws.write(store, wrapper, (list, own), first_group, segments.end)
                });
                wrapper
            }
        };
        assign_layer(
            store,
            wrapper,
            self.head(&DrawGroup::plain(Rect::from_size(size))),
        );
        Item::child(wrapper)
    }

    // Draws the recording in `wrapper`, the wrapper the command drew
    // `segments` in before, when its segments lie in the lanes of the one the
    // wrapper draws: the records keep their items, each run takes the
    // recording at its segments, and each draw group takes its head anew.
    // `false` leaves the wrapper as it was.
    fn redraw_kept(
        &self,
        store: &mut SceneStore,
        wrapper: RecordId,
        segments: &Range<u32>,
    ) -> bool {
        let Some(&first) = store.items(wrapper).first() else {
            return false;
        };
        let old = store.run(first);
        if old.segments.start != segments.start || !same_lanes(&old.recording, self.recording) {
            return false;
        }
        let recording = self.recording;
        let mut groups = recording
            .segments_in(segments)
            .filter(|segment| segment.lane == RecordLane::GroupStart)
            .map(|segment| recording.groups()[segment.start as usize]);
        self.redraw_in_place(store, wrapper, &mut groups);
        true
    }

    // Gives each run beneath `record` the recording at its segments and each
    // draw group beneath it the head of the next of `groups`, in draw order,
    // then notes the new content of `record`.
    fn redraw_in_place(
        &self,
        store: &mut SceneStore,
        record: RecordId,
        groups: &mut impl Iterator<Item = DrawGroup>,
    ) {
        for position in 0..store.items(record).len() {
            let item = store.items(record)[position];
            if let Some(child) = item.as_child() {
                let group = groups
                    .next()
                    .expect("segments in one lanes open as many groups");
                self.redraw_in_place(store, child, groups);
                assign_layer(store, child, self.head(&group));
            } else {
                store.swap_run_recording(item, Rc::clone(self.recording));
                let segments = store.run(item).segments.clone();
                draw_segments(store, item, segments);
            }
        }
        store.note_content_change(record);
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

    // Gives `record` the items `write` lists, which take the record's old
    // items in their places; the old items none took go.
    fn write_record(
        &self,
        store: &mut SceneStore,
        record: RecordId,
        write: impl FnOnce(&Self, &mut SceneStore, &mut Vec<Item>, &mut OwnDraws<'_>) -> u32,
    ) -> u32 {
        let mut old = store.lists.take();
        old.extend_from_slice(store.items(record));
        let mut list = store.lists.take();
        let mut own = OwnDraws::of(&old, &[]);
        let position = write(self, store, &mut list, &mut own);
        store.write_items(record, &list);
        own.finish(store);
        // Its runs draw another recording, whether or not its items changed.
        store.note_content_change(record);
        store.lists.give(list);
        store.lists.give(old);
        position
    }

    fn write(
        &self,
        store: &mut SceneStore,
        parent: RecordId,
        (list, own): (&mut Vec<Item>, &mut OwnDraws<'_>),
        mut position: u32,
        end: u32,
    ) -> u32 {
        let segments = self.recording.segments();
        let mut run_start = position;
        while position < end {
            let segment = segments[position as usize];
            match segment.lane {
                RecordLane::GroupStart => {
                    self.push_run(store, (list, own), run_start..position);
                    let record = own
                        .take(store, is_draw_group)
                        .and_then(Item::as_child)
                        .unwrap_or_else(|| self.group_record(store, parent));
                    position = self.write_record(store, record, |draws, store, nested, inner| {
                        draws.write(store, record, (nested, inner), position + 1, end)
                    });
                    let group = self.recording.groups()[segment.start as usize];
                    assign_layer(store, record, self.head(&group));
                    list.push(Item::child(record));
                    run_start = position;
                }
                RecordLane::GroupEnd => {
                    self.push_run(store, (list, own), run_start..position);
                    return position + 1;
                }
                RecordLane::Shapes | RecordLane::Others | RecordLane::Content => position += 1,
            }
        }
        self.push_run(store, (list, own), run_start..end);
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

    // The run of `segments`: the run in its place, which takes the
    // recording, or a new one.
    fn run_item(
        &self,
        store: &mut SceneStore,
        own: &mut OwnDraws<'_>,
        segments: Range<u32>,
    ) -> Item {
        let Some(run) = own.take(store, |store, item| is_run_of(store, item, self.id)) else {
            return store.add_run(self.run(segments));
        };
        store.swap_run_recording(run, Rc::clone(self.recording));
        draw_segments(store, run, segments);
        run
    }

    fn push_run(
        &self,
        store: &mut SceneStore,
        (list, own): (&mut Vec<Item>, &mut OwnDraws<'_>),
        segments: Range<u32>,
    ) {
        if !segments.is_empty() {
            list.push(self.run_item(store, own, segments));
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

// The draws of a node as a pass writes them: the record that takes their
// items, the commands and what they draw into.
#[derive(Clone, Copy)]
pub(super) struct NodeDraws<'a> {
    pub(super) parent: RecordId,
    pub(super) commands: NodeCommands<'a>,
    pub(super) target: DrawTarget<'a>,
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
    let node = NodeDraws {
        parent,
        commands,
        target: draws,
    };
    append_draw_nodes(
        store,
        &node,
        (&mut items, &mut OwnDraws::none()),
        DrawPass::Record(placement),
        phase,
    );
    items
}

// Writes the draws of `node` that `pass` places into `items`, items of its
// record: each draw takes the item of `own` in its place when that one drew
// it before, and a new item otherwise. Returns whether a command draws
// over the node's content, so the pass behind it tells its caller whether
// the pass over it has anything to write.
pub(super) fn append_draw_nodes(
    store: &mut SceneStore,
    node: &NodeDraws<'_>,
    (items, own): (&mut Vec<Item>, &mut OwnDraws<'_>),
    mut pass: DrawPass<'_>,
    phase: PrimitivePhase,
) -> bool {
    let commands = node.commands;
    let placement = pass.placement();
    let mut over = false;
    for (command_index, command) in commands.commands.iter().enumerate() {
        over |= matches!(
            command,
            DrawCommand::Overlay(_) | DrawCommand::WithContent(_)
        );
        let id = DrawCommandId {
            node_id: commands.node_id,
            command_index: (commands.first_index + command_index) as u32,
            placement,
        };
        let draw = (id, phase);
        if let Some(shared) = pass.take_recording(store, command, items) {
            items.push(shared_item(store, node, own, draw, shared));
            continue;
        }
        // A background takes shapes in the store's chunks, behind the
        // node's content alone.
        if let DrawCommand::Background(background) = command {
            if placement == DrawPlacement::Behind {
                items.push(background_item(store, own, background, node.target.size));
            }
            continue;
        }
        // The overlay pass skips a command that draws behind alone before
        // it looks for the command's recording.
        if !draws_in(command, placement) {
            continue;
        }
        if let Some(item) = command_item(store, node, own, (command, draw)) {
            items.push(item);
        }
    }
    over
}

// Whether `item` is the run that drew the command `id`.
fn is_run_of(store: &SceneStore, item: Item, id: DrawCommandId) -> bool {
    item.kind() == ItemKind::Run && store.run(item).command == Some(id)
}

// Whether the segments of `new` lie in the lanes of those of `old`, so each
// run and draw group of `old` names the same segments in `new`.
fn same_lanes(old: &CommandRecording, new: &CommandRecording) -> bool {
    old.segments().len() == new.segments().len()
        && (old.segments().iter())
            .zip(new.segments())
            .all(|(old, new)| old.lane == new.lane)
}

// Whether `item` is the record of a draw group.
fn is_draw_group(store: &SceneStore, item: Item) -> bool {
    item.as_child()
        .is_some_and(|record| store.kind(record) == RecordKind::DrawGroup)
}

// Whether `item` is the wrapper the grouped command `id` drew in.
fn is_wrapper_of(store: &SceneStore, item: Item, id: DrawCommandId) -> bool {
    is_draw_group(store, item)
        && item
            .as_child()
            .and_then(|record| store.items(record).first())
            .is_some_and(|first| is_run_of(store, *first, id))
}

// The item of the overlay half of a with-content command, which draws its
// segments of `shared`, the recording the behind half made: the run that
// drew the half before takes the recording, else a new run does. A half
// that opens a draw group takes a layer of its own.
#[inline(never)]
fn shared_item(
    store: &mut SceneStore,
    node: &NodeDraws<'_>,
    own: &mut OwnDraws<'_>,
    (id, phase): (DrawCommandId, PrimitivePhase),
    shared: Rc<CommandRecording>,
) -> Item {
    let segments = shared.content_split(DrawPlacement::Overlay);
    if opens_group(&shared, &segments) {
        return grouped_item(store, node, own, (id, phase), &shared, segments);
    }
    let Some(run) = own.take(store, |store, item| is_run_of(store, item, id)) else {
        return store.add_run(DrawRunNode::for_command_shared(
            phase,
            Some(id),
            shared,
            segments,
        ));
    };
    store.swap_run_recording(run, shared);
    draw_segments(store, run, segments);
    run
}

// Makes the run `item` draw `segments` of the recording it holds.
fn draw_segments(store: &mut SceneStore, item: Item, segments: Range<u32>) {
    let run = store.run_mut(item);
    run.summary = run.recording.summary_in(&segments);
    run.segments = segments;
}

// The item of a background's shapes: the shape item in its place, which
// keeps its shapes while their print stays, or a new one.
fn background_item(
    store: &mut SceneStore,
    own: &mut OwnDraws<'_>,
    background: &BackgroundDraw,
    size: Size,
) -> Item {
    let Some(item) = own.take(store, |_, item| item.kind() == ItemKind::Shape) else {
        return add_background(store, background, size);
    };
    let shape = background.shape_in(size);
    let print = background_print(shape, background.color());
    if store.shapes_print(item) != print {
        store.redraw_shapes(item, print, |recorder| background.record(shape, recorder));
    }
    item
}

// The item that draws `command`, recorded now. The run that drew the
// command before, when it lies in the draw's place, records it again: into
// its own recording when no reader holds it, else into storage the store
// keeps for the command. Without such a run the command records into that
// storage for a new run. A recording that opens a draw group takes a layer
// of its own. `None` when the command draws nothing in its placement.
#[inline(never)]
fn command_item(
    store: &mut SceneStore,
    node: &NodeDraws<'_>,
    own: &mut OwnDraws<'_>,
    (command, (id, phase)): (&DrawCommand, (DrawCommandId, PrimitivePhase)),
) -> Option<Item> {
    let size = node.target.size;
    let Some(run) = own.take(store, |store, item| is_run_of(store, item, id)) else {
        let (recording, segments) = record_command(store, command, id, size)?;
        return Some(if opens_group(&recording, &segments) {
            grouped_item(store, node, own, (id, phase), &recording, segments)
        } else {
            store.add_run(DrawRunNode::for_command_shared(
                phase,
                Some(id),
                recording,
                segments,
            ))
        });
    };
    let in_place = Rc::get_mut(&mut store.run_mut(run).recording)
        .map(|recording| record_in_place(command, id.placement, size, recording));
    let segments = match in_place {
        Some(segments) => segments,
        None => record_command(store, command, id, size).map(|(recording, segments)| {
            store.swap_run_recording(run, recording);
            segments
        }),
    };
    let Some(segments) = segments else {
        store.remove_item(run);
        return None;
    };
    if opens_group(&store.run(run).recording, &segments) {
        // The groups' runs take the recording; the run that held it goes.
        let recording = Rc::clone(&store.run(run).recording);
        store.remove_item(run);
        return Some(grouped_item(
            store,
            node,
            own,
            (id, phase),
            &recording,
            segments,
        ));
    }
    draw_segments(store, run, segments);
    Some(run)
}

// The item of the text `slices` of the node `node_id` draw at `size`: the
// text item in its place takes the text, where a text and style that stay
// keep their paint; else a new one does. `None` when the node draws no
// text. Out of line: most nodes draw no text.
#[inline(never)]
pub(super) fn text_item(
    store: &mut SceneStore,
    own: &mut OwnDraws<'_>,
    (node_id, slices): (NodeId, &ModifierNodeSlices),
    size: Size,
) -> Option<Item> {
    let parts = TextNodeParts::of(node_id, slices, size);
    let Some(item) = own.take(store, |_, item| item.kind() == ItemKind::Text) else {
        return text_node_from_parts(&parts).map(|text| store.add_text(text));
    };
    if refresh_text_node(store.text_mut(item), &parts) {
        return Some(item);
    }
    store.remove_item(item);
    None
}

// Whether `segments` of `recording` open a draw group, which a layer of its
// own places.
fn opens_group(recording: &CommandRecording, segments: &Range<u32>) -> bool {
    recording.summary().has_groups
        && recording
            .segments_in(segments)
            .any(|segment| segment.lane == RecordLane::GroupStart)
}

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
    store.recordings.record(id, |storage| {
        recording_for_placement_reusing(command, id.placement, size, storage)
    })
}

// The layer a recording's draw groups take, in a wrapper under `parent`:
// the wrapper in the draw's place, when the command drew in it before, takes
// the groups.
#[cold]
#[inline(never)]
fn grouped_item(
    store: &mut SceneStore,
    node: &NodeDraws<'_>,
    own: &mut OwnDraws<'_>,
    (id, phase): (DrawCommandId, PrimitivePhase),
    recording: &Rc<CommandRecording>,
    segments: Range<u32>,
) -> Item {
    let draws = node.target;
    let kept = own
        .take(store, |store, item| is_wrapper_of(store, item, id))
        .and_then(Item::as_child);
    GroupedDraws::new(store, id, phase, recording, draws).wrapper(
        store,
        node.parent,
        segments,
        draws.size,
        kept,
    )
}

// Lowers `commands`, the draw commands of node `node_id` of size `size`,
// into items of `record` that paint in `phase`, as the scene builder lowers
// a node's draws: runs, and layers for the draws a `DrawScope` groups.
#[cfg(feature = "layer-tree")]
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
