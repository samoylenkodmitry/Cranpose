//! Masonry widgets the gauntlet needs and Xilem has none of, each with its
//! view: a frame clock, a row that wraps, a box that holds one child (and
//! measures, scrolls or turns it), a filled bar and a sparkline.

use std::sync::OnceLock;

use perf_data::{AVATAR_COUNT, AVATAR_SIZE, GAUNTLET_SPARK_POINTS, avatar_rgba, spark_value};
use xilem::{
    Blob, Color, ImageBrush, ImageFormat, Pod, ViewCtx, WidgetView,
    core::{
        AppendVec, ElementSplice, MessageContext, MessageResult, Mut, SuperElement, View,
        ViewElement, ViewId, ViewMarker, ViewPathTracker as _, ViewSequence,
    },
    masonry::{
        accesskit::{Node, Role},
        core::{
            AccessCtx, BoxConstraints, ChildrenIds, FromDynWidget, LayoutCtx, NewWidget, NoAction,
            PaintCtx, PropertiesMut, PropertiesRef, RegisterCtx, Update, UpdateCtx, Widget,
            WidgetMut, WidgetPod,
        },
        kurbo::{Affine, BezPath, Circle, Point, Rect, RoundedRect, Size, Stroke, Vec2},
        peniko::{Fill, Gradient, ImageAlphaType, ImageData},
        properties::{BoxShadow, types::UnitPoint},
        vello::Scene,
    },
};

const CHILD: ViewId = ViewId::new(0);

/// What a widget of no children and no accessibility of its own answers.
macro_rules! leaf {
    () => {
        fn register_children(&mut self, _: &mut RegisterCtx<'_>) {}

        fn accessibility_role(&self) -> Role {
            Role::GenericContainer
        }

        fn accessibility(&mut self, _: &mut AccessCtx<'_>, _: &PropertiesRef<'_>, _: &mut Node) {}

        fn children_ids(&self) -> ChildrenIds {
            ChildrenIds::new()
        }
    };
}

// ---------------------------------------------------------------- frame clock

/// Asks Masonry for every animation frame and reports each as an action.
pub struct FrameClock;

/// One animation frame.
#[derive(Debug)]
pub struct Tick;

impl Widget for FrameClock {
    type Action = Tick;

    fn on_anim_frame(&mut self, ctx: &mut UpdateCtx<'_>, _: &mut PropertiesMut<'_>, _: u64) {
        ctx.submit_action::<Tick>(Tick);
        ctx.request_anim_frame();
    }

    fn update(&mut self, ctx: &mut UpdateCtx<'_>, _: &mut PropertiesMut<'_>, event: &Update) {
        if matches!(event, Update::WidgetAdded) {
            ctx.request_anim_frame();
        }
    }

    fn layout(
        &mut self,
        _: &mut LayoutCtx<'_>,
        _: &mut PropertiesMut<'_>,
        bc: &BoxConstraints,
    ) -> Size {
        bc.min()
    }

    fn paint(&mut self, _: &mut PaintCtx<'_>, _: &PropertiesRef<'_>, _: &mut Scene) {}

    leaf!();
}

/// Calls `on_frame` on every frame Masonry draws; it answers whether the
/// state changed.
pub fn frame_clock<State, F: Fn(&mut State) -> bool + 'static>(on_frame: F) -> FrameClockView<F> {
    FrameClockView { on_frame }
}

pub struct FrameClockView<F> {
    on_frame: F,
}

impl<F> ViewMarker for FrameClockView<F> {}
impl<State: 'static, F: Fn(&mut State) -> bool + 'static> View<State, (), ViewCtx>
    for FrameClockView<F>
{
    type Element = Pod<FrameClock>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _: &mut State) -> (Self::Element, Self::ViewState) {
        ctx.with_leaf_action_widget(|ctx| ctx.create_pod(FrameClock))
    }

    fn rebuild(
        &self,
        _: &Self,
        (): &mut (),
        _: &mut ViewCtx,
        _: Mut<'_, Self::Element>,
        _: &mut State,
    ) {
    }

    fn teardown(&self, (): &mut (), ctx: &mut ViewCtx, element: Mut<'_, Self::Element>) {
        ctx.teardown_leaf(element);
    }

    fn message(
        &self,
        (): &mut (),
        message: &mut MessageContext,
        _: Mut<'_, Self::Element>,
        state: &mut State,
    ) -> MessageResult<()> {
        match message.take_message::<Tick>() {
            Some(_) if (self.on_frame)(state) => MessageResult::Action(()),
            Some(_) => MessageResult::Nop,
            None => MessageResult::Stale,
        }
    }
}

// ---------------------------------------------------------------- holder

/// How a [`Holder`] lays out and places its one child.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hold {
    /// As big as the child; reports each new height.
    Measure,
    /// As big as the parent allows; the child as tall as it needs, moved
    /// down by `shift` and clipped to the holder.
    Viewport { shift: f64 },
    /// As big as the parent allows, the child clipped to it.
    Clip,
    /// As big as the parent allows, the child at its own size, placed at
    /// `UnitPoint` of the room left: Masonry's sized box puts its child at
    /// the top left.
    Align(UnitPoint),
    /// As big as the child, at most `max` tall, clipped to that.
    Clamp { max: f64 },
    /// As big as the child, moved by `offset` and turned `radians` about its
    /// centre: only the holder's transform changes when these do.
    Turn { offset: Vec2, radians: f64 },
    /// As big as the child, over a shadow of a box rounded by `radius`, cast
    /// `offset_y` down in `color` and blurred by `blur`.
    Shadow {
        radius: f64,
        offset_y: f64,
        blur: f64,
        color: Color,
    },
}

impl Hold {
    fn transform(self, size: Size) -> Affine {
        match self {
            Hold::Turn { offset, radians } => {
                Affine::translate(offset) * Affine::rotate_about(radians, size.to_rect().center())
            }
            Hold::Measure
            | Hold::Viewport { .. }
            | Hold::Clip
            | Hold::Align(_)
            | Hold::Clamp { .. }
            | Hold::Shadow { .. } => Affine::IDENTITY,
        }
    }
}

/// A box holding one child, placed as its [`Hold`] says.
pub struct Holder {
    child: WidgetPod<dyn Widget>,
    hold: Hold,
    /// The size of the last layout.
    size: Size,
}

/// A measured child's new height.
#[derive(Debug)]
pub struct Height(pub f64);

impl Holder {
    fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }

    fn set_hold(this: &mut WidgetMut<'_, Self>, hold: Hold) {
        let moved_only = matches!(
            (this.widget.hold, hold),
            (Hold::Turn { .. }, Hold::Turn { .. })
        );
        this.widget.hold = hold;
        if moved_only {
            this.ctx.set_transform(hold.transform(this.widget.size));
        } else {
            this.ctx.request_layout();
        }
    }
}

impl Widget for Holder {
    type Action = Height;

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        _: &mut PropertiesMut<'_>,
        bc: &BoxConstraints,
    ) -> Size {
        match self.hold {
            Hold::Viewport { shift } => {
                let size = bc.max();
                let content = BoxConstraints::new(
                    Size::new(size.width, 0.0),
                    Size::new(size.width, f64::INFINITY),
                );
                ctx.run_layout(&mut self.child, &content);
                ctx.place_child(&mut self.child, Point::new(0.0, shift));
                ctx.set_clip_path(size.to_rect());
                size
            }
            Hold::Clip => {
                let size = bc.max();
                ctx.run_layout(&mut self.child, &BoxConstraints::tight(size));
                ctx.place_child(&mut self.child, Point::ORIGIN);
                ctx.set_clip_path(size.to_rect());
                size
            }
            Hold::Align(point) => {
                let size = bc.max();
                let child = ctx.run_layout(&mut self.child, &bc.loosen());
                let room = size - child;
                let origin = point.resolve(Rect::new(0.0, 0.0, room.width, room.height));
                ctx.place_child(&mut self.child, origin);
                size
            }
            Hold::Clamp { max } => {
                let loose = BoxConstraints::new(bc.min(), Size::new(bc.max().width, f64::INFINITY));
                let child = ctx.run_layout(&mut self.child, &loose);
                let size = Size::new(child.width, child.height.min(max));
                ctx.place_child(&mut self.child, Point::ORIGIN);
                ctx.set_clip_path(size.to_rect());
                size
            }
            Hold::Measure | Hold::Turn { .. } | Hold::Shadow { .. } => {
                let size = ctx.run_layout(&mut self.child, bc);
                ctx.place_child(&mut self.child, Point::ORIGIN);
                if size != self.size {
                    self.size = size;
                    match self.hold {
                        Hold::Measure => ctx.submit_action::<Height>(Height(size.height)),
                        // Layout cannot set a transform: the holder sets its
                        // own once the layout pass is over.
                        Hold::Turn { .. } => ctx.mutate_self_later(move |mut this| {
                            let mut this = this.downcast::<Holder>();
                            let transform = this.widget.hold.transform(size);
                            this.ctx.set_transform(transform);
                        }),
                        _ => {}
                    }
                }
                size
            }
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_>, _: &PropertiesRef<'_>, scene: &mut Scene) {
        if let Hold::Shadow {
            radius,
            offset_y,
            blur,
            color,
        } = self.hold
        {
            let shadow = BoxShadow::new(color, (0.0, offset_y)).blur(blur);
            let rect = RoundedRect::from_rect(ctx.size().to_rect(), radius);
            shadow.paint(scene, Affine::IDENTITY, rect);
        }
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(&mut self, _: &mut AccessCtx<'_>, _: &PropertiesRef<'_>, _: &mut Node) {}

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }
}

/// Holds `child` as `hold` says, calling `on_height` with each new height a
/// measured child reports.
pub fn hold<State, V, F>(child: V, hold: Hold, on_height: F) -> Held<V, F>
where
    V: WidgetView<State, ()>,
    F: Fn(&mut State, f64) + 'static,
{
    Held {
        child,
        hold,
        on_height,
    }
}

/// For a holder whose child's height nobody needs.
pub fn ignore_height<State>(_: &mut State, _: f64) {}

pub struct Held<V, F> {
    child: V,
    hold: Hold,
    on_height: F,
}

impl<V, F> ViewMarker for Held<V, F> {}
impl<State, V, F> View<State, (), ViewCtx> for Held<V, F>
where
    State: 'static,
    V: WidgetView<State, ()>,
    F: Fn(&mut State, f64) + 'static,
{
    type Element = Pod<Holder>;
    type ViewState = V::ViewState;

    fn build(&self, ctx: &mut ViewCtx, state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child, child_state) = ctx.with_id(CHILD, |ctx| self.child.build(ctx, state));
        let holder = Holder {
            child: child.new_widget.erased().to_pod(),
            hold: self.hold,
            size: Size::new(-1.0, -1.0),
        };
        (
            ctx.with_action_widget(|ctx| ctx.create_pod(holder)),
            child_state,
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        child_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        state: &mut State,
    ) {
        if self.hold != prev.hold {
            Holder::set_hold(&mut element, self.hold);
        }
        ctx.with_id(CHILD, |ctx| {
            self.child.rebuild(
                &prev.child,
                child_state,
                ctx,
                Holder::child_mut(&mut element).downcast(),
                state,
            );
        });
    }

    fn teardown(
        &self,
        child_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
    ) {
        ctx.with_id(CHILD, |ctx| {
            self.child
                .teardown(child_state, ctx, Holder::child_mut(&mut element).downcast());
        });
        ctx.teardown_leaf(element);
    }

    fn message(
        &self,
        child_state: &mut Self::ViewState,
        message: &mut MessageContext,
        mut element: Mut<'_, Self::Element>,
        state: &mut State,
    ) -> MessageResult<()> {
        match message.take_first() {
            Some(CHILD) => self.child.message(
                child_state,
                message,
                Holder::child_mut(&mut element).downcast(),
                state,
            ),
            Some(_) => MessageResult::Stale,
            None => match message.take_message::<Height>() {
                Some(height) => {
                    (self.on_height)(state, height.0);
                    MessageResult::Action(())
                }
                None => MessageResult::Stale,
            },
        }
    }
}

// ---------------------------------------------------------------- flow

/// Children left to right, wrapping, `gap` apart both ways, as Compose's
/// `FlowRow`.
pub struct Flow {
    children: Vec<WidgetPod<dyn Widget>>,
    gap: f64,
}

impl Flow {
    fn insert(this: &mut WidgetMut<'_, Self>, index: usize, child: NewWidget<dyn Widget>) {
        this.widget.children.insert(index, child.to_pod());
        this.ctx.children_changed();
        this.ctx.request_layout();
    }

    fn remove(this: &mut WidgetMut<'_, Self>, index: usize) {
        let child = this.widget.children.remove(index);
        this.ctx.remove_child(child);
        this.ctx.request_layout();
    }

    fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>, index: usize) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.children[index])
    }
}

impl Widget for Flow {
    type Action = NoAction;

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        _: &mut PropertiesMut<'_>,
        bc: &BoxConstraints,
    ) -> Size {
        let width = bc.max().width;
        let loose = BoxConstraints::new(Size::ZERO, Size::new(width, f64::INFINITY));
        let (mut x, mut y, mut line) = (0.0_f64, 0.0_f64, 0.0_f64);
        for child in &mut self.children {
            let size = ctx.run_layout(child, &loose);
            if x > 0.0 && x + size.width > width {
                y += line + self.gap;
                (x, line) = (0.0, 0.0);
            }
            ctx.place_child(child, Point::new(x, y));
            x += size.width + self.gap;
            line = line.max(size.height);
        }
        bc.constrain(Size::new(width, y + line))
    }

    fn paint(&mut self, _: &mut PaintCtx<'_>, _: &PropertiesRef<'_>, _: &mut Scene) {}

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        for child in &mut self.children {
            ctx.register_child(child);
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(&mut self, _: &mut AccessCtx<'_>, _: &PropertiesRef<'_>, _: &mut Node) {}

    fn children_ids(&self) -> ChildrenIds {
        self.children.iter().map(WidgetPod::id).collect()
    }
}

/// `children` left to right, wrapping, `gap` apart both ways.
pub fn flow<State, Seq>(children: Seq, gap: f64) -> FlowView<Seq>
where
    Seq: ViewSequence<State, (), ViewCtx, FlowElement>,
{
    FlowView { children, gap }
}

pub struct FlowView<Seq> {
    children: Seq,
    gap: f64,
}

/// A child of a [`Flow`], as Xilem's sequences hand it over.
pub struct FlowElement(Pod<dyn Widget>);

/// A child of a [`Flow`] to change in place.
pub struct FlowElementMut<'w> {
    parent: WidgetMut<'w, Flow>,
    index: usize,
}

impl ViewElement for FlowElement {
    type Mut<'a> = FlowElementMut<'a>;
}

impl SuperElement<Self, ViewCtx> for FlowElement {
    fn upcast(_: &mut ViewCtx, child: Self) -> Self {
        child
    }

    fn with_downcast_val<R>(
        mut this: Mut<'_, Self>,
        f: impl FnOnce(Mut<'_, Self>) -> R,
    ) -> (Self::Mut<'_>, R) {
        let r = f(FlowElementMut {
            parent: this.parent.reborrow_mut(),
            index: this.index,
        });
        (this, r)
    }
}

impl<W: Widget + FromDynWidget + ?Sized> SuperElement<Pod<W>, ViewCtx> for FlowElement {
    fn upcast(_: &mut ViewCtx, child: Pod<W>) -> Self {
        Self(child.erased())
    }

    fn with_downcast_val<R>(
        mut this: Mut<'_, Self>,
        f: impl FnOnce(Mut<'_, Pod<W>>) -> R,
    ) -> (Self::Mut<'_>, R) {
        let r = f(Flow::child_mut(&mut this.parent, this.index).downcast());
        (this, r)
    }
}

/// Applies a sequence's changes to a [`Flow`]'s children.
struct FlowSplice<'w, 's> {
    index: usize,
    element: WidgetMut<'w, Flow>,
    scratch: &'s mut AppendVec<FlowElement>,
}

impl ElementSplice<FlowElement> for FlowSplice<'_, '_> {
    fn with_scratch<R>(&mut self, f: impl FnOnce(&mut AppendVec<FlowElement>) -> R) -> R {
        let r = f(self.scratch);
        for FlowElement(child) in self.scratch.drain() {
            Flow::insert(&mut self.element, self.index, child.new_widget);
            self.index += 1;
        }
        r
    }

    fn insert(&mut self, FlowElement(child): FlowElement) {
        Flow::insert(&mut self.element, self.index, child.new_widget);
        self.index += 1;
    }

    fn mutate<R>(&mut self, f: impl FnOnce(Mut<'_, FlowElement>) -> R) -> R {
        let r = f(FlowElementMut {
            parent: self.element.reborrow_mut(),
            index: self.index,
        });
        self.index += 1;
        r
    }

    fn skip(&mut self, n: usize) {
        self.index += n;
    }

    fn index(&self) -> usize {
        self.index
    }

    fn delete<R>(&mut self, f: impl FnOnce(Mut<'_, FlowElement>) -> R) -> R {
        let r = f(FlowElementMut {
            parent: self.element.reborrow_mut(),
            index: self.index,
        });
        Flow::remove(&mut self.element, self.index);
        r
    }
}

impl<Seq> ViewMarker for FlowView<Seq> {}
impl<State, Seq> View<State, (), ViewCtx> for FlowView<Seq>
where
    State: 'static,
    Seq: ViewSequence<State, (), ViewCtx, FlowElement>,
{
    type Element = Pod<Flow>;
    type ViewState = (Seq::SeqState, AppendVec<FlowElement>);

    fn build(&self, ctx: &mut ViewCtx, state: &mut State) -> (Self::Element, Self::ViewState) {
        let mut elements = AppendVec::default();
        let seq_state = self.children.seq_build(ctx, &mut elements, state);
        let children = elements
            .drain()
            .map(|FlowElement(child)| child.new_widget.to_pod())
            .collect();
        let widget = Flow {
            children,
            gap: self.gap,
        };
        (ctx.create_pod(widget), (seq_state, elements))
    }

    fn rebuild(
        &self,
        prev: &Self,
        (seq_state, scratch): &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        state: &mut State,
    ) {
        if self.gap != prev.gap {
            element.widget.gap = self.gap;
            element.ctx.request_layout();
        }
        let mut splice = FlowSplice {
            index: 0,
            element,
            scratch,
        };
        self.children
            .seq_rebuild(&prev.children, seq_state, ctx, &mut splice, state);
    }

    fn teardown(
        &self,
        (seq_state, scratch): &mut Self::ViewState,
        ctx: &mut ViewCtx,
        element: Mut<'_, Self::Element>,
    ) {
        let mut splice = FlowSplice {
            index: 0,
            element,
            scratch,
        };
        self.children.seq_teardown(seq_state, ctx, &mut splice);
    }

    fn message(
        &self,
        (seq_state, scratch): &mut Self::ViewState,
        message: &mut MessageContext,
        element: Mut<'_, Self::Element>,
        state: &mut State,
    ) -> MessageResult<()> {
        let mut splice = FlowSplice {
            index: 0,
            element,
            scratch,
        };
        self.children
            .seq_message(seq_state, message, &mut splice, state)
    }
}

// ---------------------------------------------------------------- drawings

/// What a [`Drawing`] paints.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Picture {
    /// A rounded track `width` wide (or as wide as it may be), filled to
    /// `share` in `fill`.
    Bar {
        width: Option<f64>,
        height: f64,
        share: f64,
        track: Color,
        fill: Color,
    },
    /// Avatar `index`, `side` wide and tall, cut to a circle.
    Avatar { index: usize, side: f64 },
    /// A card's 48-point line over a fading fill, on `frame`.
    Sparkline {
        card: usize,
        frame: u32,
        color: Color,
        height: f64,
        stroke: f64,
    },
}

/// The avatars, decoded once.
fn avatars() -> &'static [ImageBrush] {
    static AVATARS: OnceLock<Vec<ImageBrush>> = OnceLock::new();
    AVATARS.get_or_init(|| {
        (0..AVATAR_COUNT)
            .map(|index| {
                ImageBrush::new(ImageData {
                    data: Blob::from(avatar_rgba(index)),
                    format: ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::Alpha,
                    width: AVATAR_SIZE,
                    height: AVATAR_SIZE,
                })
            })
            .collect()
    })
}

/// A widget that paints a [`Picture`].
pub struct Drawing {
    picture: Picture,
}

impl Widget for Drawing {
    type Action = NoAction;

    fn layout(
        &mut self,
        _: &mut LayoutCtx<'_>,
        _: &mut PropertiesMut<'_>,
        bc: &BoxConstraints,
    ) -> Size {
        let (width, height) = match self.picture {
            Picture::Bar { width, height, .. } => (width.unwrap_or(bc.max().width), height),
            Picture::Sparkline { height, .. } => (bc.max().width, height),
            Picture::Avatar { side, .. } => (side, side),
        };
        bc.constrain(Size::new(width, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_>, _: &PropertiesRef<'_>, scene: &mut Scene) {
        let size = ctx.size();
        match self.picture {
            Picture::Bar {
                height,
                share,
                track,
                fill,
                ..
            } => {
                let radius = height / 2.0;
                let full = RoundedRect::from_rect(size.to_rect(), radius);
                scene.fill(Fill::NonZero, Affine::IDENTITY, track, None, &full);
                let filled = Rect::new(0.0, 0.0, size.width * share.clamp(0.0, 1.0), size.height);
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    fill,
                    None,
                    &RoundedRect::from_rect(filled, radius),
                );
            }
            Picture::Avatar { index, side } => {
                if let Some(avatar) = avatars().get(index) {
                    let circle = Circle::new(size.to_rect().center(), side / 2.0);
                    scene.push_clip_layer(Affine::IDENTITY, &circle);
                    scene.draw_image(avatar, Affine::scale(side / f64::from(AVATAR_SIZE)));
                    scene.pop_layer();
                }
            }
            Picture::Sparkline {
                card,
                frame,
                color,
                stroke,
                ..
            } => {
                let step = size.width / (GAUNTLET_SPARK_POINTS - 1) as f64;
                let point = |index: usize| {
                    Point::new(
                        index as f64 * step,
                        size.height * (1.0 - f64::from(spark_value(card, index, frame))),
                    )
                };
                let (mut line, mut area) = (BezPath::new(), BezPath::new());
                area.move_to((0.0, size.height));
                for index in 0..GAUNTLET_SPARK_POINTS {
                    if index == 0 {
                        line.move_to(point(index));
                    } else {
                        line.line_to(point(index));
                    }
                    area.line_to(point(index));
                }
                area.line_to((size.width, size.height));
                area.close_path();
                let fade = Gradient::new_linear((0.0, 0.0), (0.0, size.height))
                    .with_stops([color.with_alpha(0.25), color.with_alpha(0.0)]);
                scene.fill(Fill::NonZero, Affine::IDENTITY, &fade, None, &area);
                scene.stroke(&Stroke::new(stroke), Affine::IDENTITY, color, None, &line);
            }
        }
    }

    leaf!();
}

/// Paints `picture`.
pub fn drawing(picture: Picture) -> DrawingView {
    DrawingView { picture }
}

pub struct DrawingView {
    picture: Picture,
}

impl ViewMarker for DrawingView {}
impl<State: 'static> View<State, (), ViewCtx> for DrawingView {
    type Element = Pod<Drawing>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _: &mut State) -> (Self::Element, Self::ViewState) {
        (
            ctx.create_pod(Drawing {
                picture: self.picture,
            }),
            (),
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        (): &mut (),
        _: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        _: &mut State,
    ) {
        if self.picture != prev.picture {
            let resized = !matches!(
                (prev.picture, self.picture),
                (Picture::Bar { width: a, height: b, .. }, Picture::Bar { width: c, height: d, .. })
                    if a == c && b == d
            ) && !matches!(
                (prev.picture, self.picture),
                (Picture::Sparkline { height: a, .. }, Picture::Sparkline { height: b, .. }) if a == b
            ) && !matches!(
                (prev.picture, self.picture),
                (Picture::Avatar { side: a, .. }, Picture::Avatar { side: b, .. }) if a == b
            );
            element.widget.picture = self.picture;
            if resized {
                element.ctx.request_layout();
            } else {
                element.ctx.request_paint_only();
            }
        }
    }

    fn teardown(&self, (): &mut (), _: &mut ViewCtx, _: Mut<'_, Self::Element>) {}

    fn message(
        &self,
        (): &mut (),
        _: &mut MessageContext,
        _: Mut<'_, Self::Element>,
        _: &mut State,
    ) -> MessageResult<()> {
        MessageResult::Stale
    }
}
