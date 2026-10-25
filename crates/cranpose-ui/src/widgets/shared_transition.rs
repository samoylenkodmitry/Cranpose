//! Shared element transitions: an element that two contents of an
//! [`AnimatedContent`](crate::AnimatedContent) both show moves from where
//! the content that goes has it to where the content that comes has it, as
//! Compose's `SharedTransitionLayout` and `Modifier.sharedElement` do.
//!
//! A [`SharedTransitionLayout`] keeps the [`SharedElement`]s composed in it.
//! While a change runs and both contents hold an element of one key, both
//! copies hide in place and the layout draws the incoming content's element
//! above everything, at a box between the two resting boxes that follows
//! the incoming content's progress, also when a gesture seeks it. A frame
//! measures that one box again; nothing recomposes.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    CompositionLocal, CompositionLocalProvider, DisposableEffect, MutableState, NodeId,
    RuntimeHandle, compositionLocalOf, internal::FrameCallbackRegistration, remember,
    with_current_composer, with_key,
};
use cranpose_ui_graphics::{Rect, Size, WindowCoordinates};
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use crate::{
    composable,
    modifier::{GraphicsLayer, Modifier},
    widgets::{
        animated_content::EntryMotion,
        box_widget::{Box, BoxSpec},
        layout::Layout,
    },
};

/// Where one content of an `AnimatedContent` sits, for the shared elements
/// composed in it.
#[derive(Clone)]
pub(crate) struct SharedEntry {
    pub(crate) motion: Rc<EntryMotion>,
    /// The content's box, its transition's layer included.
    pub(crate) entry: Rc<Cell<WindowCoordinates>>,
    /// The `AnimatedContent` the content sits in, where the content rests.
    pub(crate) container: Rc<Cell<WindowCoordinates>>,
}

/// One content, compared by identity.
impl PartialEq for SharedEntry {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.motion, &other.motion)
    }
}

/// What a flight between two copies carries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sharing {
    /// The coming content, in a box that moves between the two.
    Element,
    /// Both contents in that box: the going one fades out and the coming one
    /// fades in.
    Bounds,
}

/// One shared element in the composition.
struct Instance {
    key: Rc<str>,
    sharing: Sharing,
    entry: SharedEntry,
    element: Rc<Cell<WindowCoordinates>>,
    content: RefCell<Rc<dyn Fn()>>,
}

impl Instance {
    /// The element's box where its content rests, in window pixels: its
    /// drawn box without its content's transition.
    fn resting(&self) -> Option<Rect> {
        let element = self.element.get();
        if element.size.width <= 0.0 || element.size.height <= 0.0 {
            return None;
        }
        let into_entry = element
            .local_to_window
            .then(self.entry.entry.get().local_to_window.inverse()?);
        Some(
            into_entry
                .then(self.entry.container.get().local_to_window)
                .bounds_for_rect(Rect::from_size(element.size)),
        )
    }
}

/// An element in flight: the incoming content's copy, and the copy it
/// moves from.
struct Flight {
    to: Rc<Instance>,
    from: Rc<Instance>,
}

struct Registry {
    runtime: RuntimeHandle,
    instances: RefCell<Vec<Rc<Instance>>>,
    /// Bumped when an element comes or goes, which starts or ends a flight.
    revision: MutableState<u64>,
    /// The layout's own box, where the flying copies are placed.
    layout: Rc<Cell<WindowCoordinates>>,
    /// The boxes of the copies in flight, measured again each frame.
    flying_nodes: RefCell<Vec<NodeId>>,
    watcher: RefCell<Option<FrameCallbackRegistration>>,
}

impl Registry {
    fn bump(&self) {
        self.revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    fn flight(&self, key: &str) -> Option<Flight> {
        let instances = self.instances.borrow();
        let of_key = || instances.iter().filter(|instance| &*instance.key == key);
        let to = of_key().find(|instance| !instance.entry.motion.leaving.get())?;
        let from = of_key().find(|instance| instance.entry.motion.leaving.get())?;
        Some(Flight {
            to: Rc::clone(to),
            from: Rc::clone(from),
        })
    }

    fn flying_keys(&self) -> Vec<Rc<str>> {
        let mut keys: Vec<Rc<str>> = Vec::new();
        for instance in self.instances.borrow().iter() {
            if !keys.contains(&instance.key) {
                keys.push(Rc::clone(&instance.key));
            }
        }
        keys.retain(|key| self.flight(key).is_some());
        keys
    }

    /// The flying copy's box in the layout's pixels, `None` until both
    /// copies were laid out.
    fn flying_box(&self, key: &str) -> Option<Rect> {
        let flight = self.flight(key)?;
        let (from, to) = (flight.from.resting()?, flight.to.resting()?);
        let progress = flight.to.entry.motion.progress().clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * progress;
        let window = Rect {
            x: lerp(from.x, to.x),
            y: lerp(from.y, to.y),
            width: lerp(from.width, to.width),
            height: lerp(from.height, to.height),
        };
        let into_layout = self.layout.get().local_to_window.inverse()?;
        Some(into_layout.bounds_for_rect(window))
    }
}

/// Measures the flying copies again each frame while any is in flight.
fn watch(registry: &Rc<Registry>) {
    if registry.watcher.borrow().is_some() || registry.flying_nodes.borrow().is_empty() {
        return;
    }
    let weak = Rc::downgrade(registry);
    let registration = registry.runtime.frame_clock().with_frame_nanos(move |_| {
        let Some(registry) = weak.upgrade() else {
            return;
        };
        registry.watcher.borrow_mut().take();
        for &node in registry.flying_nodes.borrow().iter() {
            crate::schedule_measure_repass(node);
        }
        crate::request_render_invalidation();
        watch(&registry);
    });
    *registry.watcher.borrow_mut() = Some(registration);
}

#[derive(Clone)]
pub(crate) struct RegistryHandle(Rc<Registry>);

impl PartialEq for RegistryHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

fn local_registry() -> CompositionLocal<Option<RegistryHandle>> {
    crate::thread_state::with_environment(|locals| {
        locals
            .shared_transition
            .get_or_init(|| compositionLocalOf(|| None))
            .clone()
    })
}

fn local_entry() -> CompositionLocal<Option<SharedEntry>> {
    crate::thread_state::with_environment(|locals| {
        locals
            .shared_entry
            .get_or_init(|| compositionLocalOf(|| None))
            .clone()
    })
}

/// Whether an `AnimatedContent` composed here sits in a
/// [`SharedTransitionLayout`], and so reports where its contents are.
pub(crate) fn in_shared_transition() -> bool {
    local_registry().current().is_some()
}

/// Composes `content` as one content of an `AnimatedContent`, for the
/// shared elements in it.
#[allow(non_snake_case)]
pub(crate) fn ProvideSharedEntry(entry: SharedEntry, content: impl FnOnce()) {
    let local = local_entry();
    CompositionLocalProvider(vec![local.provides(Some(entry))], content);
}

/// Gives the [`SharedElement`]s composed in `content` their transitions,
/// and draws the elements in flight above `content`.
///
/// Compose's `SharedTransitionLayout`.
#[composable]
pub fn SharedTransitionLayout(modifier: Modifier, content: impl FnMut() + 'static) {
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let registry = remember(move || {
        RegistryHandle(Rc::new(Registry {
            revision: MutableState::with_runtime(0, runtime.clone()),
            runtime,
            instances: RefCell::new(Vec::new()),
            layout: Rc::default(),
            flying_nodes: RefCell::new(Vec::new()),
            watcher: RefCell::new(None),
        }))
    })
    .with(Clone::clone);
    let local = local_registry();
    let overlay = registry.clone();
    let mut content = content;
    Box(
        modifier.report_window_coordinates(Rc::clone(&registry.0.layout)),
        BoxSpec::new(),
        move || {
            CompositionLocalProvider(vec![local.provides(Some(registry.clone()))], &mut content);
            Flights(overlay.clone());
        },
    );
}

/// The copies in flight, drawn after the content and so above it.
#[composable(no_skip)]
fn Flights(registry: RegistryHandle) {
    registry.0.revision.get();
    let keys = registry.0.flying_keys();
    registry.0.flying_nodes.borrow_mut().clear();
    for key in keys {
        let Some(flight) = registry.0.flight(&key) else {
            continue;
        };
        let coming = Rc::clone(&flight.to.content.borrow());
        let going = (flight.to.sharing == Sharing::Bounds)
            .then(|| Rc::clone(&flight.from.content.borrow()));
        let motion = Rc::clone(&flight.to.entry.motion);
        let policy = FlightPolicy {
            registry: registry.clone(),
            key: Rc::clone(&key),
        };
        let nodes = Rc::clone(&registry.0);
        with_key(&key, move || {
            let node = Layout(Modifier::empty(), policy, move || match &going {
                None => coming(),
                Some(going) => {
                    Crossfaded(Rc::clone(&motion), false, Rc::clone(going));
                    Crossfaded(Rc::clone(&motion), true, Rc::clone(&coming));
                }
            });
            nodes.flying_nodes.borrow_mut().push(node);
        });
    }
    watch(&registry.0);
}

/// One content of a [`SharedBounds`] flight, faded in when it is `coming`
/// and out when it goes, as far as the change has run.
#[composable(no_skip)]
fn Crossfaded(motion: Rc<EntryMotion>, coming: bool, content: Rc<dyn Fn()>) {
    Box(
        Modifier::empty().graphics_layer(move || {
            let progress = motion.progress().clamp(0.0, 1.0);
            GraphicsLayer {
                alpha: if coming { progress } else { 1.0 - progress },
                ..GraphicsLayer::default()
            }
        }),
        BoxSpec::new(),
        move || content(),
    );
}

/// Places the flying copy at its box between the two resting boxes.
#[derive(Clone, PartialEq)]
struct FlightPolicy {
    registry: RegistryHandle,
    key: Rc<str>,
}

impl MeasurePolicy for FlightPolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let flying = self.registry.0.flying_box(&self.key);
        let placements = measurables
            .iter()
            .map(|measurable| {
                let Some(flying) = flying else {
                    // Not laid out yet: nowhere to draw it.
                    let placeable = measurable.measure(Constraints::tight(0.0, 0.0));
                    placeable.place(0.0, 0.0);
                    return Placement::new(placeable.node_id(), 0.0, 0.0, 0);
                };
                let placeable = measurable.measure(Constraints::tight(flying.width, flying.height));
                placeable.place(flying.x, flying.y);
                Placement::new(placeable.node_id(), flying.x, flying.y, 0)
            })
            .collect();
        // Takes no room: the copy is placed in the layout's pixels, outside
        // this box at the layout's top start.
        MeasureResult::new(
            Size::new(constraints.min_width, constraints.min_height),
            placements,
        )
    }

    fn min_intrinsic_width(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_width(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _: &[Box<dyn Measurable>], _: f32) -> f32 {
        0.0
    }
}

/// An element that two contents of an `AnimatedContent` in a
/// [`SharedTransitionLayout`] share by `key`: while the change between
/// them runs, it moves and resizes from the box the going content gives it
/// to the box the coming content gives it, drawn above both. Outside a
/// change, or outside a `SharedTransitionLayout`, it is `content` in a box
/// with `modifier`.
///
/// Compose's `Modifier.sharedElement(rememberSharedContentState(key), ..)`.
/// The flying copy is the coming content's `content` composed once more,
/// so state it remembers starts afresh in flight.
#[composable]
pub fn SharedElement(key: impl Into<String>, modifier: Modifier, content: impl Fn() + 'static) {
    shared(key.into(), Sharing::Element, modifier, Rc::new(content));
}

/// Bounds that two contents of an `AnimatedContent` in a
/// [`SharedTransitionLayout`] share by `key` while their contents differ, a
/// card and the screen it opens: while the change between them runs, a box
/// moves and resizes from the going content's bounds to the coming
/// content's, drawn above both, and in it the going `content` fades out as
/// the coming one fades in, each laid out to the box. Outside a change, or
/// outside a `SharedTransitionLayout`, it is `content` in a box with
/// `modifier`.
///
/// Compose's `Modifier.sharedBounds(rememberSharedContentState(key), ..)`
/// with its default fades and `RemeasureToBounds`. Both copies in flight
/// are their contents composed once more.
#[composable]
pub fn SharedBounds(key: impl Into<String>, modifier: Modifier, content: impl Fn() + 'static) {
    shared(key.into(), Sharing::Bounds, modifier, Rc::new(content));
}

/// The body of [`SharedElement`] and [`SharedBounds`].
fn shared(key: String, sharing: Sharing, modifier: Modifier, content: Rc<dyn Fn()>) {
    let (Some(registry), Some(entry)) = (local_registry().current(), local_entry().current())
    else {
        Box(modifier, BoxSpec::new(), move || content());
        return;
    };
    let key: Rc<str> = key.into();
    let instance = remember({
        let key = Rc::clone(&key);
        move || {
            Rc::new(Instance {
                key,
                sharing,
                entry,
                element: Rc::default(),
                content: RefCell::new(Rc::new(|| {})),
            })
        }
    })
    .with(Rc::clone);
    *instance.content.borrow_mut() = Rc::clone(&content);
    DisposableEffect((), {
        let (registry, instance) = (registry.clone(), Rc::clone(&instance));
        move |scope| {
            registry.0.instances.borrow_mut().push(Rc::clone(&instance));
            registry.0.bump();
            scope.on_dispose(move || {
                registry
                    .0
                    .instances
                    .borrow_mut()
                    .retain(|other| !Rc::ptr_eq(other, &instance));
                registry.0.bump();
            })
        }
    });
    let hiding = registry;
    Box(
        modifier
            .report_window_coordinates(Rc::clone(&instance.element))
            .graphics_layer(move || {
                hiding.0.revision.get();
                GraphicsLayer {
                    alpha: if hiding.0.flight(&key).is_some() {
                        0.0
                    } else {
                        1.0
                    },
                    ..GraphicsLayer::default()
                }
            }),
        BoxSpec::new(),
        move || content(),
    );
}
