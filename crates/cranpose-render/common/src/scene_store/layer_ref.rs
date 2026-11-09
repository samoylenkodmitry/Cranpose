// One record of a store as its readers see it: the layer a renderer draws,
// hit testing walks and the inspector shows, under the names a layer of the
// render graph had.

use cranpose_core::{NodeId, RecordId, RecordRef, SceneUpdate, StoreId};
use cranpose_ui::{GraphicsLayer, Point, Rect, RenderEffect, Size};
use cranpose_ui_graphics::{BlendMode, ColorFilter, CompositingStrategy, DrawPrimitive};

use super::{
    Extent, Item, ItemKind, LayerProps, Reach, RecordFlags, RecordKind, SceneStore, ShapeRef,
};
use crate::{
    graph::{
        CachePolicy, DrawPrimitiveNode, DrawRunNode, HitTestNode, IsolationReasons, PrimitivePhase,
        ProjectiveTransform, TextPrimitiveNode,
    },
    raster_cache::LayerRasterCacheHashes,
};

/// One record of a [`SceneStore`]: a layer, a wrapper, a draw group or a
/// leaf drawn in its parent's layer.
#[derive(Clone, Copy)]
pub struct LayerRef<'a> {
    pub(super) store: &'a SceneStore,
    pub(super) id: RecordId,
}

/// One item of a record, in draw order.
#[derive(Clone, Copy)]
pub enum ItemRef<'a> {
    /// A child layer: a node's layer, a wrapper or a draw group.
    Layer(LayerRef<'a>),
    /// A layout node drawn in this record's layer, placed by a translation.
    Leaf(LayerRef<'a>),
    /// A draw primitive with its clip.
    Draw(&'a DrawPrimitiveNode),
    /// A text.
    Text(&'a TextPrimitiveNode),
    /// A run of one draw command's primitives.
    Run(&'a DrawRunNode),
    /// Shapes the store keeps in a chunk it shares among shape items.
    Shape(ShapeRef<'a>),
}

/// What a record was at one scene update: equal values mean the record, or
/// the part of it they were taken of, did not change in between.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecordChange {
    store: StoreId,
    record: RecordRef,
    update: SceneUpdate,
}

impl<'a> LayerRef<'a> {
    #[inline]
    pub(crate) fn new(store: &'a SceneStore, id: RecordId) -> Self {
        Self { store, id }
    }

    /// The store that holds the record.
    #[inline]
    pub fn store(self) -> &'a SceneStore {
        self.store
    }

    /// The record's id in its store.
    #[inline]
    pub fn id(self) -> RecordId {
        self.id
    }

    /// What the record draws for.
    #[inline]
    pub fn kind(self) -> RecordKind {
        self.store.tree[self.id.index()].kind
    }

    /// The layout node of a node's layer or leaf; `None` for a wrapper, a
    /// draw group and an overlay.
    #[inline]
    pub fn node_id(self) -> Option<NodeId> {
        match self.kind() {
            RecordKind::Layer | RecordKind::Leaf => self.owner(),
            RecordKind::Wrapper | RecordKind::DrawGroup => None,
        }
    }

    /// The node whose outer draws a wrapper holds.
    #[inline]
    pub fn wraps(self) -> Option<NodeId> {
        match self.kind() {
            RecordKind::Wrapper => self.owner(),
            RecordKind::Layer | RecordKind::Leaf | RecordKind::DrawGroup => None,
        }
    }

    /// The layout node the record draws for, whatever its kind.
    #[inline]
    pub fn owner(self) -> Option<NodeId> {
        self.store.node(self.id)
    }

    /// The record that holds this one.
    #[inline]
    pub fn parent(self) -> Option<Self> {
        self.store.tree[self.id.index()]
            .parent
            .map(|parent| Self::new(self.store, parent))
    }

    /// The properties the record keeps beyond its fixed-size parts.
    #[inline]
    pub fn props(self) -> Option<&'a LayerProps> {
        self.store.props(self.id)
    }

    /// The record's flags.
    #[inline]
    pub fn flags(self) -> RecordFlags {
        self.store.tree[self.id.index()].flags
    }

    /// The node's rect, `(0, 0, size)`, where its pointer handlers are.
    #[inline]
    pub fn node_rect(self) -> Rect {
        self.store.geo[self.id.index()].node_rect()
    }

    /// The node's size.
    #[inline]
    pub fn size(self) -> Size {
        let [width, height] = self.store.geo[self.id.index()].size;
        Size { width, height }
    }

    #[expect(
        clippy::inline_always,
        reason = "the collect walk and the update call it for every record; out of line the gauntlet frame took 2% more instructions"
    )]
    /// The bounds of the layer, where its clip cuts, its transforms pivot and
    /// an offscreen pass draws it.
    #[inline(always)]
    pub fn local_bounds(self) -> Rect {
        if !self.flags().contains(RecordFlags::HAS_LAYER_BOUNDS) {
            return self.node_rect();
        }
        self.props()
            .and_then(|props| props.layer_bounds)
            .unwrap_or_else(|| self.node_rect())
    }

    /// The node's rect when it differs from [`Self::local_bounds`].
    #[inline]
    pub fn node_bounds(self) -> Option<Rect> {
        self.props()
            .and_then(|props| props.layer_bounds)
            .map(|_| self.node_rect())
    }

    /// The record's top left in its parent's layer: its placement and the
    /// parent's content offset. A leaf's transform moves it there.
    #[inline]
    pub fn origin(self) -> Point {
        self.store.origin(self.id)
    }

    /// Maps the record's space to its parent's content space, with the
    /// placement, the graphics layer transform and the parent's content
    /// offset applied.
    #[inline]
    pub fn transform_to_parent(self) -> ProjectiveTransform {
        self.store.transform_to_parent(self.id)
    }

    /// Offset of the node's content box from its origin, where its children
    /// are placed.
    #[inline]
    pub fn content_offset(self) -> Point {
        self.store.content_offset(self.id)
    }

    /// Where layout placed the record within its parent's content, before
    /// the parent's content offset.
    #[inline]
    pub fn origin_in_parent(self) -> Point {
        self.store.origin_in_parent(self.id)
    }

    /// Whether the record lies in a motion context such as an animated
    /// scroll, where a renderer must not snap its content rigidly to the
    /// pixel grid.
    #[inline]
    pub fn motion_context_animated(self) -> bool {
        self.flags().contains(RecordFlags::MOTION_ANIMATED)
    }

    /// Whether the record moves its content as one translated block, so a
    /// renderer may snap that content to the pixel grid as a whole.
    #[inline]
    pub fn translated_content_context(self) -> bool {
        self.flags().contains(RecordFlags::TRANSLATED_CONTENT)
    }

    /// Translation of the content in a translated content context.
    #[inline]
    pub fn translated_content_offset(self) -> Point {
        self.props()
            .map(|props| props.translated_content_offset)
            .unwrap_or_default()
    }

    /// The graphics layer properties; defaults when none were set.
    #[inline]
    pub fn graphics_layer(self) -> &'a GraphicsLayer {
        self.store
            .graphics_layer(self.id)
            .unwrap_or(&GraphicsLayer::DEFAULT)
    }

    /// Whether the record cuts its content to its local bounds.
    #[inline]
    pub fn clip_to_bounds(self) -> bool {
        self.flags().contains(RecordFlags::CLIP_TO_BOUNDS)
    }

    /// Pointer handling of the node.
    #[inline]
    pub fn hit_test(self) -> Option<&'a HitTestNode> {
        self.props().and_then(|props| props.hit_test.as_ref())
    }

    /// Whether the record or a record beneath it handles pointers.
    #[inline]
    pub fn has_hit_targets(self) -> bool {
        super::facts::has_hit_targets(self)
    }

    /// Whether the record's subtree publishes live window origins: a text
    /// field's popup anchor, a scroll container's viewport rect.
    #[inline]
    pub fn has_origin_sinks(self) -> bool {
        super::facts::has_origin_sinks(self)
    }

    /// Whether the record or a record beneath it is a scene, a mesh surface
    /// or a 3D transform.
    #[inline]
    pub fn has_space(self) -> bool {
        super::facts::has_space(self)
    }

    /// Where the record's subtree draws, in its own space: what its items
    /// draw and its children's extents, within its clip. A change beneath
    /// the record since [`SceneStore::refresh_reach`] leaves it unknown.
    pub fn reach(self) -> Reach {
        if self.flags().contains(RecordFlags::REACH_STALE) {
            return Reach::Unknown;
        }
        self.store.reach(self.id.index())
    }

    /// Where the record's own draws reach, beside its children's, in its
    /// own space and within its clip. A change beneath the record since
    /// [`SceneStore::refresh_reach`] leaves it unknown.
    pub fn own_reach(self) -> Reach {
        if self.flags().contains(RecordFlags::REACH_STALE) {
            return Reach::Unknown;
        }
        let marks = self.store.warm[self.id.index()].own_marks;
        super::own_marks::reach(self.store.marks.span(marks)).within(self.visual_clip_rect())
    }

    /// What the record draws in its parent's space.
    pub fn extent(self) -> Extent {
        super::reach::extent(self)
    }

    /// Whether everything the record's subtree draws lies within its local
    /// bounds, give or take [`crate::graph::CONTAINED_DRAW_SLACK`].
    #[inline]
    pub fn draws_within_bounds(self) -> bool {
        super::facts::contained(self)
    }

    /// The reasons the record is drawn through its own offscreen buffer.
    #[inline]
    pub fn isolation(self) -> IsolationReasons {
        self.props()
            .map(|props| props.isolation)
            .unwrap_or_default()
    }

    /// Whether the record's own composition needs its content on a texture
    /// of its own: a reason to draw offscreen, or a graphics layer that
    /// composites apart.
    #[inline]
    pub fn needs_surface(self) -> bool {
        // A record with neither properties nor a graphics layer draws in
        // place.
        let kept = self
            .flags()
            .only(RecordFlags::HAS_PROPS | RecordFlags::HAS_GRAPHICS_LAYER);
        kept != RecordFlags::default() && self.composes_apart()
    }

    #[inline(never)]
    fn composes_apart(self) -> bool {
        let graphics = self.graphics_layer();
        let isolation = self.isolation();
        isolation.explicit_offscreen
            || isolation.effect
            || isolation.blend_mode
            || isolation.group_opacity
            || graphics.compositing_strategy == CompositingStrategy::Offscreen
            || crate::layer_composition::layer_requires_isolation(graphics)
    }

    /// The phase a draw group paints in among its node's own draws; `None`
    /// for every other kind.
    pub fn draw_phase(self) -> Option<PrimitivePhase> {
        (self.kind() == RecordKind::DrawGroup).then(|| {
            if self.flags().contains(RecordFlags::AFTER_CHILDREN) {
                PrimitivePhase::AfterChildren
            } else {
                PrimitivePhase::BeforeChildren
            }
        })
    }

    /// Whether a renderer may keep the record's pixels between frames.
    #[inline]
    pub fn cache_policy(self) -> CachePolicy {
        self.props()
            .map(|props| props.cache_policy)
            .unwrap_or_default()
    }

    #[expect(
        clippy::inline_always,
        reason = "the collect walk and the update call it for every record; out of line the gauntlet frame took 2% more instructions"
    )]
    /// The rectangle that cuts the content and pointer hits: the local
    /// bounds when the record clips to them or its graphics layer clips.
    #[inline(always)]
    pub fn clip_rect(self) -> Option<Rect> {
        (self.clip_to_bounds() || self.graphics_layer().clip).then(|| self.local_bounds())
    }

    /// Where the record's own shadow is cut: its bounds when it clips its
    /// content to them. A graphics-layer clip leaves the shadow whole.
    #[inline]
    pub fn shadow_clip(self) -> Option<Rect> {
        self.clip_to_bounds().then(|| self.local_bounds())
    }

    #[expect(
        clippy::inline_always,
        reason = "the collect walk and the update call it for every record; out of line the gauntlet frame took 2% more instructions"
    )]
    /// Where the record's drawing is cut: its clip or, when it composites
    /// through an offscreen buffer for its alpha or by request, its bounds.
    #[inline(always)]
    pub fn visual_clip_rect(self) -> Option<Rect> {
        let clip = self.clip_rect();
        if clip.is_some() {
            return clip;
        }
        let isolation = self.isolation();
        (isolation.group_opacity || isolation.explicit_offscreen).then(|| self.local_bounds())
    }

    /// The render effect applied to the record's content.
    #[inline]
    pub fn effect(self) -> Option<&'a RenderEffect> {
        self.graphics_layer().render_effect.as_ref()
    }

    /// The effect applied to what lies behind the record.
    #[inline]
    pub fn backdrop(self) -> Option<&'a RenderEffect> {
        self.graphics_layer().backdrop_effect.as_ref()
    }

    /// Alpha of the record from 0.0 (transparent) to 1.0 (opaque).
    #[inline]
    pub fn opacity(self) -> f32 {
        self.graphics_layer().alpha
    }

    /// How the record composites onto what lies beneath it.
    #[inline]
    pub fn blend_mode(self) -> BlendMode {
        self.graphics_layer().blend_mode
    }

    /// The color filter applied to the record's content.
    #[inline]
    pub fn color_filter(self) -> Option<&'a ColorFilter> {
        self.graphics_layer().color_filter.as_ref()
    }

    /// Hash of everything the record draws into its target. Equal hashes
    /// mean cached pixels are still valid.
    pub fn target_content_hash(self) -> u64 {
        self.raster_cache_hashes().target_content
    }

    /// Hash of the record's content like [`Self::target_content_hash`],
    /// without the translated content offset, so it stays equal while the
    /// content only moves.
    pub fn motion_source_content_hash(self) -> u64 {
        super::content_hash::motion_source_content_hash(self)
    }

    /// Hash of the record's render effect, for keying cached effect output.
    pub fn effect_hash(self) -> u64 {
        self.raster_cache_hashes().effect
    }

    fn raster_cache_hashes(self) -> LayerRasterCacheHashes {
        let index = self.id.index();
        let subtree_rev = self.store.tree[index].subtree_rev;
        self.store.hashes.hashes(index, subtree_rev, || {
            super::content_hash::raster_cache_hashes(self)
        })
    }

    /// Whether what the record draws changed in scene update `update` and in
    /// the one before it, so a raster cache keyed on its content would miss
    /// again.
    pub fn content_churns(self, update: SceneUpdate) -> bool {
        self.props()
            .is_some_and(|props| props.content_run.churns(update))
    }

    /// What the record and its subtree were at their last change; their
    /// placement in the parent left out.
    pub fn change(self) -> RecordChange {
        self.change_at(self.store.subtree_rev(self.id))
    }

    /// What the record itself was at its last change: its properties, draws
    /// and items.
    pub fn own_change(self) -> RecordChange {
        self.change_at(self.store.content_rev(self.id))
    }

    fn change_at(self, update: SceneUpdate) -> RecordChange {
        RecordChange {
            store: self.store.id(),
            record: self.store.record_ref(self.id),
            update,
        }
    }

    /// How many items the record holds.
    #[inline]
    pub fn item_count(self) -> usize {
        self.store.items(self.id).len()
    }

    /// The record's items in draw order.
    #[inline]
    pub fn items(self) -> impl DoubleEndedIterator<Item = ItemRef<'a>> + ExactSizeIterator + 'a {
        let store = self.store;
        store
            .items(self.id)
            .iter()
            .map(move |item| ItemRef::of(store, *item))
    }

    /// The record's own draws in draw order: its items without its children.
    #[inline]
    pub fn own_draws(self) -> impl Iterator<Item = ItemRef<'a>> + 'a {
        let store = self.store;
        store
            .items(self.id)
            .iter()
            .filter(|item| item.kind() != ItemKind::Child)
            .map(move |item| ItemRef::of(store, *item))
    }

    /// Every string the record and the records beneath it paint, in draw
    /// order.
    pub fn painted_text(self) -> Vec<String> {
        fn collect(layer: LayerRef<'_>, out: &mut Vec<String>) {
            for item in layer.items() {
                match item {
                    ItemRef::Text(text) => out.push(text.render_text.text().to_owned()),
                    ItemRef::Run(run) => {
                        for primitive in run.primitives() {
                            if let DrawPrimitive::Text(text) = primitive {
                                out.push(text.text.to_string());
                            }
                        }
                    }
                    ItemRef::Layer(child) | ItemRef::Leaf(child) => collect(child, out),
                    ItemRef::Draw(_) | ItemRef::Shape(_) => {}
                }
            }
        }

        let mut out = Vec::new();
        collect(self, &mut out);
        out
    }

    /// The child records among the items, in draw order.
    #[inline]
    pub fn children(self) -> impl DoubleEndedIterator<Item = Self> + 'a {
        let store = self.store;
        store
            .items(self.id)
            .iter()
            .filter_map(move |item| item.as_child().map(|child| Self::new(store, child)))
    }
}

impl<'a> ItemRef<'a> {
    #[expect(
        clippy::inline_always,
        reason = "the collect walk and the update call it for every record; out of line the gauntlet frame took 2% more instructions"
    )]
    #[inline(always)]
    pub(crate) fn of(store: &'a SceneStore, item: Item) -> Self {
        match item.kind() {
            ItemKind::Child => {
                let child = LayerRef::new(
                    store,
                    RecordId::from_index(item.index()).expect("a child item names a record row"),
                );
                match child.kind() {
                    RecordKind::Leaf => Self::Leaf(child),
                    RecordKind::Layer | RecordKind::Wrapper | RecordKind::DrawGroup => {
                        Self::Layer(child)
                    }
                }
            }
            ItemKind::Draw => Self::Draw(store.draw(item)),
            ItemKind::Text => Self::Text(store.text(item)),
            ItemKind::Run => Self::Run(store.run(item)),
            ItemKind::Shape => Self::Shape(store.shapes(item)),
        }
    }
}
