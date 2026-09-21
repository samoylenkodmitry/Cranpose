//! Root surfaces: what the shell keeps per window.
//!
//! The composition is one tree. Each window the app shows is a root in that
//! tree: the composition root for the primary window, a node carrying
//! `Modifier::window_root` for every other. A [`RootSurface`] is the shell's
//! bookkeeping for one such root: the renderer that draws it, the viewport it
//! draws into, the pointer that hovers it, the gesture it tracks, the
//! snapshots a test or a platform reads for it, and the dirt that says whether
//! it owes the display a frame. A [`SurfaceMut`] borrows one surface together
//! with the app, which is how a platform delivers a window's events and reads
//! a window's frame.

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use cranpose_core::{NodeId, collections::map::HashSet};
use cranpose_foundation::{PointerButtons, PointerSource, RotaryScrollEvent};
use cranpose_render_common::Renderer;
use cranpose_ui::{
    LayoutTree, PlatformTextInputHandler, SemanticsTree, pointer_icon_session::PointerIconState,
};
use cranpose_ui_graphics::{Point, PointerIcon, Size};
use web_time::Instant;

use crate::{
    AppShell, DevOverlayControl, FramePacingMode, FrameRatePreference, FrameSchedule,
    FrameScheduler, FrameUpdateResult, PlatformFrameDriver, ShellApp,
    hit_path_tracker::{HitPathTracker, PointerId},
};

/// Names a root surface of the app: the primary window, or a window root
/// declared with [`Modifier::window_root`](cranpose_ui::Modifier::window_root)
/// by the id given there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RootId {
    /// The composition root, drawn in the app's first window.
    Primary,
    /// The node carrying `Modifier::window_root` with this id.
    Window(u64),
}

/// The shell's bookkeeping for one root.
///
/// The primary surface draws the composition root. A window surface draws the
/// window root node registered under its id, and draws nothing while that
/// node is not in the tree. Everything here is what an OS owns per window:
/// the framebuffer, the viewport, the pointer, the cursor image, the soft
/// keyboard, the frame the window is asked for.
pub struct RootSurface<R: Renderer> {
    pub(crate) id: RootId,
    pub(crate) root: Option<NodeId>,
    pub(crate) renderer: R,
    pub(crate) cursor: (f32, f32),
    pub(crate) viewport: (f32, f32),
    pub(crate) buffer_size: (u32, u32),
    pub(crate) layout_tree: Option<LayoutTree>,
    pub(crate) semantics_tree: Option<SemanticsTree>,
    pub(crate) frame_rate_preference: FrameRatePreference,
    pub(crate) scene_dirty: bool,
    pub(crate) scoped_layout_scene_nodes: Vec<NodeId>,
    pub(crate) retained_visual_nodes: HashSet<NodeId>,
    pub(crate) is_dirty: bool,
    pub(crate) buttons_pressed: PointerButtons,
    pub(crate) pointer_source: PointerSource,
    pub(crate) hit_path_tracker: HitPathTracker,
    pub(crate) hovered_nodes: Vec<NodeId>,
    pub(crate) on_rotary_scroll: Option<Rc<dyn Fn(RotaryScrollEvent) -> bool>>,
    pub(crate) dev_overlay_controls: Vec<DevOverlayControl>,
    pub(crate) inspector: crate::inspector::DeveloperInspector,
    pub(crate) dev_overlay_text: String,
    pub(crate) dev_overlay_last_refresh: Option<Instant>,
    pub(crate) dev_overlay_viewport: Option<Size>,
    pub(crate) frame_scheduler: FrameScheduler,
    pub(crate) pointer_icon: PointerIconState,
    pub(crate) last_update: FrameUpdateResult,
    pub(crate) frame_owed: bool,
    pub(crate) screen_origin: Option<Point>,
}

impl<R: Renderer> RootSurface<R> {
    pub(crate) fn new(
        id: RootId,
        renderer: R,
        buffer_size: (u32, u32),
        viewport: (f32, f32),
    ) -> Self {
        Self {
            id,
            root: None,
            renderer,
            cursor: (0.0, 0.0),
            viewport,
            buffer_size,
            layout_tree: None,
            semantics_tree: None,
            frame_rate_preference: FrameRatePreference::default(),
            scene_dirty: true,
            scoped_layout_scene_nodes: Vec::new(),
            retained_visual_nodes: HashSet::new(),
            is_dirty: true,
            buttons_pressed: PointerButtons::NONE,
            pointer_source: PointerSource::Unknown,
            hit_path_tracker: HitPathTracker::new(),
            hovered_nodes: Vec::new(),
            on_rotary_scroll: None,
            dev_overlay_controls: Vec::new(),
            inspector: crate::inspector::DeveloperInspector::default(),
            dev_overlay_text: String::new(),
            dev_overlay_last_refresh: None,
            dev_overlay_viewport: None,
            frame_scheduler: FrameScheduler::default(),
            pointer_icon: PointerIconState::new(),
            last_update: FrameUpdateResult::default(),
            frame_owed: false,
            screen_origin: None,
        }
    }

    pub(crate) fn root_node(&self, app: &ShellApp) -> Option<NodeId> {
        match self.id {
            RootId::Primary => app.composition.root(),
            RootId::Window(_) => self.root,
        }
    }

    pub(crate) fn owns_nodes_under(&self, window_root: Option<NodeId>) -> bool {
        match self.id {
            RootId::Primary => window_root.is_none(),
            RootId::Window(_) => self.root.is_some() && self.root == window_root,
        }
    }

    pub(crate) fn viewport_size(&self) -> Size {
        Size {
            width: self.viewport.0,
            height: self.viewport.1,
        }
    }

    pub(crate) fn screen_point_inside(&self, screen: Point) -> Option<Point> {
        let origin = self.screen_origin?;
        let local = Point {
            x: screen.x - origin.x,
            y: screen.y - origin.y,
        };
        (local.x >= 0.0
            && local.y >= 0.0
            && local.x <= self.viewport.0
            && local.y <= self.viewport.1)
            .then_some(local)
    }

    pub(crate) fn set_root(&mut self, root: Option<NodeId>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.forget_snapshots();
        self.scoped_layout_scene_nodes.clear();
        self.retained_visual_nodes.clear();
        self.hit_path_tracker.clear();
        self.hovered_nodes.clear();
        self.buttons_pressed = PointerButtons::NONE;
        self.scene_dirty = true;
        self.is_dirty = true;
    }

    pub(crate) fn forget_snapshots(&mut self) {
        self.layout_tree = None;
        self.semantics_tree = None;
    }

    pub(crate) fn has_active_pointer_gesture(&self) -> bool {
        self.buttons_pressed != PointerButtons::NONE
            && self.hit_path_tracker.has_path(PointerId::PRIMARY)
    }

    pub(crate) fn renderer_warmup_due(&self, app: &ShellApp) -> bool {
        self.renderer.needs_frame_warmup() && !app.runtime.runtime_handle().has_frame_callbacks()
    }

    pub(crate) fn needs_redraw_in_context(&self, app: &ShellApp) -> bool {
        app.has_stale_work_in_context()
            || self.is_dirty
            || self.scene_dirty
            || self.renderer_warmup_due(app)
    }

    pub(crate) fn compute_frame_schedule(
        &self,
        app: &ShellApp,
        surfaces_dirty: bool,
    ) -> FrameSchedule {
        let app_context = Rc::clone(&app.app_context);
        let (needs_update, needs_frame) = app_context.enter(|| {
            let needs_frame = self.is_dirty
                || self.scene_dirty
                || app.wants_frame_in_context()
                || self.has_active_pointer_gesture()
                || self.renderer_warmup_due(app);
            (app.needs_ui_update_in_context(surfaces_dirty), needs_frame)
        });
        FrameSchedule {
            needs_update,
            needs_frame,
            next_deadline: app.next_event_time(),
        }
    }

    pub(crate) fn invalidate_dev_overlay_text(&mut self) {
        self.dev_overlay_text.clear();
        self.dev_overlay_last_refresh = None;
        self.dev_overlay_viewport = None;
    }

    pub(crate) fn dev_overlay_control_center(&self, mode: FramePacingMode) -> Option<(f32, f32)> {
        self.dev_overlay_controls
            .iter()
            .find(|control| control.mode == mode)
            .map(|control| {
                (
                    control.bounds.x + control.bounds.width * 0.5,
                    control.bounds.y + control.bounds.height * 0.5,
                )
            })
    }

    pub(crate) fn layout_tree_in_context(&mut self, app: &mut ShellApp) -> Option<&LayoutTree> {
        if self.layout_tree.is_none() {
            let root = self.root_node(app)?;
            let mut applier = app.composition.applier_mut();
            match cranpose_ui::build_layout_tree_from_applier(&mut applier, root) {
                Ok(layout_tree) => {
                    self.layout_tree = layout_tree;
                }
                Err(err) => {
                    log::debug!("failed to build layout snapshot: {err}");
                    return None;
                }
            }
        }
        self.layout_tree.as_ref()
    }

    pub(crate) fn semantics_tree_in_context(
        &mut self,
        app: &mut ShellApp,
    ) -> Option<&SemanticsTree> {
        if !app.semantics_enabled && !self.inspector.state.open {
            return None;
        }
        self.semantics_tree_for_input(app)
    }

    pub(crate) fn semantics_tree_for_input(
        &mut self,
        app: &mut ShellApp,
    ) -> Option<&SemanticsTree> {
        let root = self.root_node(app)?;
        let semantics_dirty = {
            let mut applier = app.composition.applier_mut();
            cranpose_ui::tree_needs_semantics(&mut *applier, root).unwrap_or_else(|err| {
                log::debug!("failed to check semantics dirty status for root #{root}: {err}");
                true
            })
        };
        if self.semantics_tree.is_none() || semantics_dirty {
            let mut applier = app.composition.applier_mut();
            match cranpose_ui::build_semantics_tree_from_applier(&mut applier, root) {
                Ok(semantics_tree) => {
                    self.semantics_tree = semantics_tree;
                }
                Err(err) => {
                    log::debug!("failed to build semantics snapshot: {err}");
                    return None;
                }
            }
        }
        self.semantics_tree.as_ref()
    }
}

#[derive(Default)]
pub(crate) struct TextInputRoutes {
    active: Option<RootId>,
    shown: Option<RootId>,
    handlers: Vec<(RootId, Rc<dyn PlatformTextInputHandler>)>,
}

impl TextInputRoutes {
    pub(crate) fn active(&self) -> RootId {
        self.active.unwrap_or(RootId::Primary)
    }

    pub(crate) fn set_active(&mut self, root: RootId) {
        self.active = Some(root);
    }

    pub(crate) fn set_handler(&mut self, root: RootId, handler: Rc<dyn PlatformTextInputHandler>) {
        self.remove(root);
        self.handlers.push((root, handler));
    }

    pub(crate) fn remove(&mut self, root: RootId) {
        self.handlers.retain(|(id, _)| *id != root);
        if self.shown == Some(root) {
            self.shown = None;
        }
    }

    fn handler(&self, root: RootId) -> Option<Rc<dyn PlatformTextInputHandler>> {
        self.handlers
            .iter()
            .find(|(id, _)| *id == root)
            .map(|(_, handler)| Rc::clone(handler))
    }

    fn take_show_target(&mut self) -> Option<Rc<dyn PlatformTextInputHandler>> {
        let active = self.active();
        let handler = self.handler(active);
        if handler.is_some() {
            self.shown = Some(active);
        }
        handler
    }

    fn take_hide_target(&mut self) -> Option<Rc<dyn PlatformTextInputHandler>> {
        let target = self.shown.take().unwrap_or_else(|| self.active());
        self.handler(target)
    }
}

pub(crate) struct TextInputRouter {
    pub(crate) routes: Rc<RefCell<TextInputRoutes>>,
}

impl PlatformTextInputHandler for TextInputRouter {
    fn show_keyboard(&self) {
        let handler = self.routes.borrow_mut().take_show_target();
        if let Some(handler) = handler {
            handler.show_keyboard();
        }
    }

    fn hide_keyboard(&self) {
        let handler = self.routes.borrow_mut().take_hide_target();
        if let Some(handler) = handler {
            handler.hide_keyboard();
        }
    }
}

pub(crate) fn partition_nodes_by_surface<R: Renderer>(
    app: &mut ShellApp,
    surfaces: &[RootSurface<R>],
    nodes: Vec<NodeId>,
) -> Vec<Vec<NodeId>> {
    let mut buckets: Vec<Vec<NodeId>> = surfaces.iter().map(|_| Vec::new()).collect();
    let Some(primary_root) = app.composition.root() else {
        return buckets;
    };
    let mut applier = app.composition.applier_mut();
    for node in nodes {
        let Some(node) = applier.scene_node_attached_to(node, primary_root) else {
            continue;
        };
        let owner = cranpose_ui::nearest_window_root(&mut applier, node);
        if let Some(index) = surfaces
            .iter()
            .position(|surface| surface.owns_nodes_under(owner))
        {
            buckets[index].push(node);
        }
    }
    buckets
}

/// One surface borrowed together with its shell: the handle a platform
/// delivers a window's events through and reads a window's frame from.
///
/// [`AppShell::surface`] hands one out per root. Every method of [`AppShell`]
/// that names no root acts on the primary surface through the same code, so
/// a single-window platform never sees this type.
pub struct SurfaceMut<'a, R: Renderer> {
    pub(crate) shell: &'a mut AppShell<R>,
    pub(crate) index: usize,
}

impl<'a, R> SurfaceMut<'a, R>
where
    R: Renderer,
    R::Error: Debug,
{
    pub(crate) fn new(shell: &'a mut AppShell<R>, index: usize) -> Self {
        Self { shell, index }
    }

    /// The whole app, for what a window's event needs beyond its surface:
    /// the clipboard, the dev options, a debug report.
    pub fn shell(&mut self) -> &mut AppShell<R> {
        self.shell
    }

    pub(crate) fn shell_app(&mut self) -> &mut ShellApp {
        &mut self.shell.app
    }

    pub(crate) fn shell_app_ref(&self) -> &ShellApp {
        &self.shell.app
    }

    pub(crate) fn surface(&self) -> &RootSurface<R> {
        &self.shell.surfaces[self.index]
    }

    pub(crate) fn surface_mut(&mut self) -> &mut RootSurface<R> {
        &mut self.shell.surfaces[self.index]
    }

    pub(crate) fn parts(&mut self) -> (&mut ShellApp, &mut RootSurface<R>) {
        let shell = &mut *self.shell;
        (&mut shell.app, &mut shell.surfaces[self.index])
    }

    /// Which root this surface draws.
    pub fn id(&self) -> RootId {
        self.surface().id
    }

    /// The node this surface draws from, when it has one.
    pub fn root(&self) -> Option<NodeId> {
        let surface = self.surface();
        surface.root_node(self.shell_app_ref())
    }

    /// The renderer that draws this surface.
    pub fn renderer(&mut self) -> &mut R {
        &mut self.surface_mut().renderer
    }

    /// The scene this surface last built.
    pub fn scene(&self) -> &R::Scene {
        self.surface().renderer.scene()
    }

    /// Sets the logical size this surface lays out and draws into.
    ///
    /// The primary surface's viewport is the composition root's constraints.
    /// A window surface's viewport is what its renderer draws into; the
    /// window root lays out to the size its descriptor reports, which the
    /// platform keeps equal to this. The next update lays out and renders;
    /// [`AppShell::set_viewport`] additionally runs that frame at once.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.surface_mut().viewport = (width, height);
        match self.id() {
            RootId::Primary => self.shell_app().request_forced_layout_pass(),
            RootId::Window(_) => {
                if let Some(root) = self.root() {
                    let app_context = Rc::clone(&self.shell_app_ref().app_context);
                    app_context.enter(|| cranpose_ui::schedule_measure_repass(root));
                }
                self.shell_app().request_layout_pass();
            }
        }
        self.surface_mut().scene_dirty = true;
        self.mark_dirty();
    }

    /// Tells the shell where the window drawing this surface sits on the
    /// screen, in logical pixels, so pointer events can carry a
    /// [`screen_position`](cranpose_foundation::PointerEvent::screen_position).
    /// A platform sets it when the window moves and before it delivers a
    /// pointer sample; `None` says the platform does not know.
    pub fn set_screen_origin(&mut self, origin: Option<Point>) {
        self.surface_mut().screen_origin = origin;
    }

    /// Where the window drawing this surface sits on the screen, as the
    /// platform last said.
    pub fn screen_origin(&self) -> Option<Point> {
        self.surface().screen_origin
    }

    /// The logical size this surface draws into.
    pub fn viewport_size(&self) -> (f32, f32) {
        self.surface().viewport
    }

    /// Sets the physical size of this surface's framebuffer.
    pub fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.surface_mut().buffer_size = (width, height);
    }

    /// The physical size of this surface's framebuffer.
    pub fn buffer_size(&self) -> (u32, u32) {
        self.surface().buffer_size
    }

    /// Marks this surface as needing a redraw.
    pub fn mark_dirty(&mut self) {
        self.surface_mut().is_dirty = true;
    }

    /// Whether this surface owes the display a frame: stale pixels, or a
    /// renderer that has not warmed its swapchain yet. See
    /// [`AppShell::needs_redraw`].
    pub fn needs_redraw(&self) -> bool {
        let app_context = Rc::clone(&self.shell_app_ref().app_context);
        app_context.enter(|| self.surface().needs_redraw_in_context(self.shell_app_ref()))
    }

    /// Whether a primary-button gesture that started on this surface is
    /// still in progress.
    pub fn has_active_pointer_gesture(&self) -> bool {
        self.surface().has_active_pointer_gesture()
    }

    /// What the update and frame produced for this surface the last time
    /// the app updated.
    pub fn last_update_result(&self) -> FrameUpdateResult {
        self.surface().last_update
    }

    /// Whether an update since the platform last presented this surface
    /// changed its pixels. An update runs for the whole app, so the update a
    /// platform ran for one window may have drawn another; this is how the
    /// other window learns it has a frame to show.
    pub fn frame_owed(&self) -> bool {
        self.surface().frame_owed
    }

    /// [`Self::frame_owed`], cleared: the platform is about to present.
    pub fn take_frame_owed(&mut self) -> bool {
        std::mem::take(&mut self.surface_mut().frame_owed)
    }

    fn compute_frame_schedule(&self) -> FrameSchedule {
        self.surface()
            .compute_frame_schedule(self.shell_app_ref(), self.shell.any_surface_dirty())
    }

    /// The frame this surface asks its platform for, recorded for
    /// [`Self::frame_scheduler_snapshot`].
    pub fn frame_schedule(&self) -> FrameSchedule {
        let schedule = self.compute_frame_schedule();
        self.surface().frame_scheduler.record(schedule);
        schedule
    }

    /// Computes this surface's frame schedule and applies it to `driver`.
    pub fn schedule_platform_frame<D>(&self, driver: &D) -> FrameSchedule
    where
        D: PlatformFrameDriver + ?Sized,
    {
        let schedule = self.compute_frame_schedule();
        self.surface().frame_scheduler.schedule(schedule, driver);
        schedule
    }

    /// The schedule this surface last recorded.
    pub fn frame_scheduler_snapshot(&self) -> FrameSchedule {
        self.surface().frame_scheduler.snapshot()
    }

    /// Sets how the platform should vote the display's frame rate for the
    /// window showing this surface. See [`AppShell::set_frame_rate_preference`].
    pub fn set_frame_rate_preference(&mut self, preference: FrameRatePreference) {
        self.surface_mut().frame_rate_preference = preference;
    }

    /// This surface's display frame-rate preference.
    pub fn frame_rate_preference(&self) -> FrameRatePreference {
        self.surface().frame_rate_preference
    }

    /// Where this surface's dev overlay draws the control for `mode`, in
    /// logical pixels. See [`AppShell::dev_overlay_control_center`].
    pub fn dev_overlay_control_center(&self, mode: FramePacingMode) -> Option<(f32, f32)> {
        self.surface().dev_overlay_control_center(mode)
    }

    pub(crate) fn dev_overlay_press(&mut self, x: f32, y: f32) -> bool {
        if !self.shell_app_ref().dev_options.frame_pacing_controls {
            return false;
        }
        let Some(mode) = self
            .surface()
            .dev_overlay_controls
            .iter()
            .find(|control| control.bounds.contains(x, y))
            .map(|control| control.mode)
        else {
            return false;
        };
        self.shell().set_frame_pacing_mode(mode);
        true
    }

    /// Runs `block` with this surface's layout snapshot, built on demand.
    pub fn with_layout_tree<T>(&mut self, block: impl FnOnce(Option<&LayoutTree>) -> T) -> T {
        let (app, surface) = self.parts();
        let app_context = Rc::clone(&app.app_context);
        app_context.enter(|| block(surface.layout_tree_in_context(app)))
    }

    /// Runs `block` with this surface's semantics snapshot, built on demand;
    /// `None` while semantics are disabled.
    pub fn with_semantics_tree<T>(&mut self, block: impl FnOnce(Option<&SemanticsTree>) -> T) -> T {
        let (app, surface) = self.parts();
        let app_context = Rc::clone(&app.app_context);
        app_context.enter(|| block(surface.semantics_tree_in_context(app)))
    }

    /// The pointer icon the platform has not applied to this surface's
    /// window yet. See [`AppShell::take_pointer_icon_change`].
    pub fn take_pointer_icon_change(&self) -> Option<PointerIcon> {
        self.surface().pointer_icon.take_change()
    }

    /// Offers this surface's pointer icon to the platform again. See
    /// [`AppShell::refresh_pointer_icon`].
    pub fn refresh_pointer_icon(&self) {
        self.surface().pointer_icon.refresh()
    }

    /// Installs the platform text input for the window showing this
    /// surface. Keyboard requests reach the handler of the surface the
    /// platform last called active, and a hide reaches the handler that
    /// showed. See [`AppShell::set_platform_text_input`].
    pub fn set_platform_text_input(&mut self, handler: Rc<dyn PlatformTextInputHandler>) {
        let id = self.id();
        let app = self.shell_app();
        app.text_input_routes.borrow_mut().set_handler(id, handler);
        app.install_text_input_router();
    }

    /// Makes this the surface the platform considers focused: the one the
    /// soft keyboard belongs to. Pointer presses do this on their own.
    pub fn activate(&mut self) {
        let id = self.id();
        self.shell_app()
            .text_input_routes
            .borrow_mut()
            .set_active(id);
    }
}
