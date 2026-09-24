use std::rc::Rc;

use cranpose_app_shell::{RootId, SurfaceMut};
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{Point, Size, WindowRootDescriptor};

use crate::{
    embed::{EmbedError, SurfaceSize},
    embed_frame::{FrameTarget, changed_rect},
    embed_overlay::HostOverlayRoot,
    embed_protocol::{
        FrameUpdate, SurfaceId, WINDOW_ALWAYS_ON_TOP, WINDOW_DECORATED, WINDOW_RESIZABLE,
        WINDOW_SHADOW, WINDOW_TAKES_FOCUS, WINDOW_TRANSPARENT, WindowSpec,
    },
    native_window::{
        NativeWindowEvents, NativeWindowOptions, NativeWindowPositionOrigin, NativeWindowRequest,
        NativeWindowRootHandle, WindowFocus, WindowState,
    },
};

pub(crate) const MAX_FRAMES_IN_FLIGHT: u32 = 2;

pub(crate) struct WindowSurface {
    pub(crate) revision: u64,
    pub(crate) spec: WindowSpec,
    pub(crate) events: NativeWindowEvents,
    pub(crate) state: Option<WindowState>,
    pub(crate) root: NativeWindowRootHandle,
    pub(crate) host_position: Option<(f32, f32)>,
}

impl WindowSurface {
    pub(crate) fn from_request(surface: SurfaceId, request: &NativeWindowRequest) -> Self {
        Self {
            revision: request.revision,
            spec: window_spec(surface, &request.options),
            events: request.events.clone(),
            state: request.state,
            root: Rc::clone(&request.root),
            host_position: None,
        }
    }

    pub(crate) fn content_size(&self) -> Size {
        Size::new(self.spec.width, self.spec.height)
    }

    pub(crate) fn set_content_size(&self, size: Size) {
        self.root.set_size(size);
        if let Some(state) = self.state {
            state.set_size(size);
            state.set_frame_size(size);
        }
    }

    pub(crate) fn set_presented(&self, presented: bool) {
        if let Some(state) = self.state {
            state.set_presented(presented);
        }
    }

    pub(crate) fn host_moved(&mut self, x: f32, y: f32) {
        self.host_position = Some((x, y));
        if let Some(state) = self.state {
            state.set_position(Some(Point { x, y }));
        }
        if let Some(on_moved) = &self.events.on_moved {
            on_moved(x, y);
        }
    }

    pub(crate) fn host_resized(&self, size: Size) {
        self.set_content_size(size);
        if let Some(on_resized) = &self.events.on_resized {
            on_resized(size.width, size.height);
        }
    }

    pub(crate) fn close_requested(&self) {
        if let Some(on_close_requested) = &self.events.on_close_requested {
            on_close_requested();
        }
    }

    pub(crate) fn refresh(&mut self, request: &NativeWindowRequest) -> Option<WindowSpec> {
        if request.revision == self.revision {
            return None;
        }
        self.revision = request.revision;
        self.events = request.events.clone();
        self.state = request.state;
        let mut spec = window_spec(self.spec.surface, &request.options);
        if spec == self.spec {
            return None;
        }
        self.spec = spec.clone();
        if spec.position == self.host_position {
            spec.position = None;
        }
        Some(spec)
    }
}

pub(crate) fn window_spec(surface: SurfaceId, options: &NativeWindowOptions) -> WindowSpec {
    let flags = [
        (options.decorations, WINDOW_DECORATED),
        (options.transparent, WINDOW_TRANSPARENT),
        (options.resizable, WINDOW_RESIZABLE),
        (options.always_on_top, WINDOW_ALWAYS_ON_TOP),
        (options.shadow, WINDOW_SHADOW),
        (options.focus != WindowFocus::Never, WINDOW_TAKES_FOCUS),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |flags, (_, bit)| flags | bit);
    WindowSpec {
        surface,
        title: options.title.clone(),
        position: options.x.zip(options.y),
        relative_to_host: options.position_origin == NativeWindowPositionOrigin::HostWindow,
        width: options.width,
        height: options.height,
        flags,
    }
}

pub(crate) enum SurfaceKind {
    Primary,
    Window(WindowSurface),
    Overlay(Rc<dyn WindowRootDescriptor>),
}

impl SurfaceKind {
    pub(crate) fn transparent(&self) -> bool {
        match self {
            Self::Primary => false,
            Self::Window(window) => window.spec.flags & WINDOW_TRANSPARENT != 0,
            Self::Overlay(_) => true,
        }
    }

    pub(crate) fn set_content_size(&self, size: Size) {
        match self {
            Self::Primary => {}
            Self::Window(window) => window.host_resized(size),
            Self::Overlay(descriptor) => {
                if let Some(overlay) = descriptor.as_any().downcast_ref::<HostOverlayRoot>() {
                    overlay.set_size(size);
                }
            }
        }
    }
}

pub(crate) struct SurfaceSlot {
    pub(crate) id: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) kind: SurfaceKind,
    target: FrameTarget,
    size: SurfaceSize,
    drawn: Vec<u8>,
    shown: Vec<u8>,
    dirty: bool,
    visible: bool,
    next_frame_id: u32,
    in_flight: u32,
}

impl SurfaceSlot {
    pub(crate) fn new(
        id: SurfaceId,
        root: RootId,
        kind: SurfaceKind,
        device: &wgpu::Device,
        size: SurfaceSize,
    ) -> Self {
        Self {
            id,
            root,
            kind,
            target: FrameTarget::new(device, size.width, size.height),
            size,
            drawn: Vec::new(),
            shown: Vec::new(),
            dirty: true,
            visible: true,
            next_frame_id: 1,
            in_flight: 0,
        }
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        if visible && !self.visible {
            self.dirty = true;
        }
        self.visible = visible;
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn wants_frame(&self) -> bool {
        self.visible && self.dirty
    }

    pub(crate) fn has_room(&self) -> bool {
        self.visible && self.in_flight < MAX_FRAMES_IN_FLIGHT
    }

    pub(crate) fn acknowledged(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }

    pub(crate) fn resize(&mut self, device: &wgpu::Device, size: SurfaceSize) -> bool {
        if size == self.size {
            return false;
        }
        if (size.width, size.height) != self.target.size() {
            self.target = FrameTarget::new(device, size.width, size.height);
            self.shown.clear();
        }
        self.size = size;
        self.dirty = true;
        true
    }

    pub(crate) fn produce(
        &mut self,
        surface: &mut SurfaceMut<'_, WgpuRenderer>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Option<FrameUpdate<'_>>, EmbedError> {
        let owed = surface.take_frame_owed();
        if !(self.dirty || owed || surface.needs_redraw()) {
            return Ok(None);
        }
        let (width, height) = self.target.size();
        surface
            .renderer()
            .render(self.target.texture(), self.target.view(), width, height)
            .map_err(|error| EmbedError::Render(format!("{error:?}")))?;
        self.target.read_into(device, queue, &mut self.drawn)?;
        self.dirty = false;
        let Some(rect) = changed_rect(&self.shown, &self.drawn, width, height) else {
            return Ok(None);
        };
        std::mem::swap(&mut self.shown, &mut self.drawn);
        let frame_id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1);
        self.in_flight += 1;
        Ok(Some(FrameUpdate {
            surface: self.id,
            frame_id,
            buffer_width: width,
            buffer_height: height,
            rect,
            pixels: &self.shown,
        }))
    }
}

#[cfg(test)]
#[path = "tests/embed_surfaces_tests.rs"]
mod tests;
