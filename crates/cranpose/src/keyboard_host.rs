// The system keyboard's board, shared by Android's input method service and
// iOS's keyboard extension. The platform hands over a window to draw into,
// touches and the focused field. The board composes in a `GuestView` and
// draws into the window's surface on the GPU.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose_app_shell::{TouchRouter, TouchSample};
use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_services::{HapticsRef, local_haptics};
use cranpose_ui::{EdgeInsets, PointerSource, Size, local_safe_area_insets};

use crate::{
    AppSettings, GpuContext, GuestView, GuestViewError,
    input_method::{EditorInfo, InputConnection, InputMethod, InputSession},
    wgpu_surface::SurfaceFrame,
};

// What a platform's keyboard window gives the shared host.
pub(crate) trait KeyboardWindow: Sized + 'static {
    type Error: std::error::Error + 'static;

    // Whether `other` is this window, which keeps its surface at a new size.
    fn same(&self, other: &Self) -> bool;

    // The instance the board's GPU opens through.
    fn instance(settings: &AppSettings) -> wgpu::Instance;

    // The device and queue the board draws with, on `adapter`.
    fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), Self::Error>;

    // A surface on this window, configured at `size` pixels.
    fn surface(
        &self,
        gpu: &GpuContext,
        size: (u32, u32),
    ) -> Result<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration), Self::Error>;

    // Asks the platform for a frame. Any thread may call it.
    fn request_frame();

    // The haptics the board reads through `local_haptics`.
    fn haptics() -> HapticsRef;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum KeyboardError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Platform(E),
    #[error("no GPU adapter for the keyboard: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("the keyboard's view: {0}")]
    View(#[from] GuestViewError),
    #[error("the application names no keyboard: AppLauncher::with_input_method")]
    NoKeyboard,
}

// When the platform shows the board's next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameRequest {
    // At the next display frame.
    Next,
    // After this long, unless input comes first.
    After(Duration),
    // When input comes.
    Idle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

// A value the platform sets and the composition reads as state.
#[derive(Clone, Default)]
struct Published<T: Copy + 'static> {
    value: Rc<Cell<T>>,
    state: Rc<Cell<Option<MutableState<T>>>>,
}

impl<T: Copy + PartialEq + 'static> Published<T> {
    fn set(&self, value: T) {
        self.value.set(value);
        if let Some(state) = self.state.get()
            && state.get_non_reactive() != value
        {
            state.set(value);
        }
    }

    // The state of the value in the composition that calls it.
    fn state(&self) -> MutableState<T> {
        let initial = self.value.get();
        let state = rememberMutableStateOf(|| initial);
        self.state.set(Some(state));
        state
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Field {
    editor: EditorInfo,
    session: InputSession,
}

struct Target<W> {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    // The surface draws into this window, so it lives as long as the surface.
    window: W,
}

struct Host<W> {
    gpu: GpuContext,
    target: Option<Target<W>>,
    // The format the view draws in, the display view of the surface's format.
    format: wgpu::TextureFormat,
    view: GuestView,
    touches: TouchRouter<u64>,
    density: f32,
}

// The keyboard's part of the main thread's state.
pub(crate) struct KeyboardState<W> {
    // The settings of a process that has a keyboard.
    settings: RefCell<Option<AppSettings>>,
    host: RefCell<Option<Host<W>>>,
    // The focused field and its turn with the keyboard.
    field: Published<Field>,
    // The edges of the board that system bars draw over.
    insets: Published<EdgeInsets>,
}

impl<W> Default for KeyboardState<W> {
    fn default() -> Self {
        Self {
            settings: RefCell::default(),
            host: RefCell::default(),
            field: Published::default(),
            insets: Published::default(),
        }
    }
}

impl<W: KeyboardWindow> KeyboardState<W> {
    // Keeps the settings of a process that has a keyboard.
    pub(crate) fn install(&self, settings: AppSettings) {
        if settings.input_method.is_some() {
            *self.settings.borrow_mut() = Some(settings);
        }
    }

    pub(crate) fn input_method(&self) -> Option<InputMethod> {
        self.settings
            .borrow()
            .as_ref()
            .and_then(|settings| settings.input_method)
    }

    // The board's height in logical pixels on a screen of `screen`.
    pub(crate) fn height(&self, screen: Size) -> Option<f32> {
        self.input_method()
            .map(|keyboard| (keyboard.height)(screen))
    }

    // A field took the keyboard.
    pub(crate) fn start_input(&self, editor: EditorInfo) {
        let session = self.field.value.get().session.next();
        self.field.set(Field { editor, session });
    }

    // The focused field changed what it asks for, such as a capital next.
    pub(crate) fn update_editor(&self, editor: EditorInfo) {
        let session = self.field.value.get().session;
        self.field.set(Field { editor, session });
    }

    // iOS draws its own bar below a keyboard extension, so only Android has
    // insets to give.
    #[cfg(target_os = "android")]
    pub(crate) fn set_insets(&self, insets: EdgeInsets) {
        self.insets.set(insets);
    }

    // Draws into `window` from now on, at `size` pixels and `density` pixels
    // per logical pixel.
    pub(crate) fn surface_changed(
        &self,
        window: W,
        size: (u32, u32),
        density: f32,
    ) -> Result<(), KeyboardError<W::Error>> {
        let settings = self.settings.borrow();
        let settings = settings.as_ref().ok_or(KeyboardError::NoKeyboard)?;
        let keyboard = settings.input_method.ok_or(KeyboardError::NoKeyboard)?;
        let mut slot = self.host.borrow_mut();
        if let Some(host) = slot.as_mut() {
            match host.target.as_mut() {
                // The same window at a new size keeps its surface: a window
                // takes one producer at a time.
                Some(target) if target.window.same(&window) => {
                    target.config.width = size.0;
                    target.config.height = size.1;
                    target.surface.configure(&host.gpu.device, &target.config);
                    host.show(size, density)?;
                    return Ok(());
                }
                // A new window: the old surface lets go of its window first.
                Some(_) => host.target = None,
                None => {}
            }
        }
        let gpu = match slot.as_ref() {
            Some(host) => host.gpu.clone(),
            None => new_gpu::<W>(settings)?,
        };
        let (surface, config) = window
            .surface(&gpu, size)
            .map_err(KeyboardError::Platform)?;
        let format = crate::surface_format::display_surface_view_format(config.format);
        let target = Target {
            surface,
            config,
            window,
        };
        match slot.as_mut() {
            Some(host) if host.format == format => {
                host.show(size, density)?;
                host.target = Some(target);
            }
            _ => {
                let mut view = GuestView::new(
                    settings,
                    &gpu,
                    format,
                    size.0,
                    size.1,
                    density,
                    self.root(keyboard.content),
                )?;
                view.shell().set_frame_waker(W::request_frame);
                *slot = Some(Host {
                    gpu,
                    target: Some(target),
                    format,
                    view,
                    touches: TouchRouter::default(),
                    density,
                });
            }
        }
        Ok(())
    }

    // The window went away: the board draws nothing until the next one.
    pub(crate) fn surface_destroyed(&self) {
        if let Some(host) = self.host.borrow_mut().as_mut() {
            host.view.set_visible(false);
            host.target = None;
        }
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        if let Some(host) = self.host.borrow_mut().as_mut() {
            host.view.set_visible(visible && host.target.is_some());
        }
    }

    // Draws a frame when the board changed and says when the next is due.
    pub(crate) fn frame(&self) -> FrameRequest {
        let mut slot = self.host.borrow_mut();
        let Some(host) = slot.as_mut() else {
            return FrameRequest::Idle;
        };
        let owed = host
            .target
            .as_mut()
            .is_some_and(|target| draw(&mut host.view, target, &host.gpu));
        let schedule = host.view.frame_schedule();
        if owed || schedule.needs_frame || schedule.needs_update {
            FrameRequest::Next
        } else {
            schedule
                .next_deadline
                .map_or(FrameRequest::Idle, |deadline| {
                    FrameRequest::After(deadline.saturating_duration_since(Instant::now()))
                })
        }
    }

    // A touch at `x`, `y` pixels in the board's window.
    pub(crate) fn touch(&self, pointer: u64, phase: TouchPhase, x: f32, y: f32) {
        let mut slot = self.host.borrow_mut();
        let Some(host) = slot.as_mut() else {
            return;
        };
        let sample = TouchSample::new(x / host.density, y / host.density);
        let shell = host.view.shell();
        shell.set_pointer_source(PointerSource::Touch);
        let finger = Some(pointer);
        match phase {
            TouchPhase::Down => {
                host.touches.press(shell, finger, sample);
            }
            TouchPhase::Move => {
                host.touches.moved(shell, finger, sample);
            }
            TouchPhase::Up => {
                host.touches.release(shell, finger, sample);
            }
            TouchPhase::Cancel => {
                host.touches
                    .cancel(shell, finger, Some((sample.x, sample.y)));
            }
        }
    }

    fn root(&self, content: fn()) -> impl FnMut() + 'static {
        let field = self.field.clone();
        let insets = self.insets.clone();
        let haptics = W::haptics();
        move || {
            let field = field.state().get();
            let insets = insets.state();
            cranpose_core::CompositionLocalProvider(
                [
                    local_safe_area_insets().provides_computed(move |_| insets.get()),
                    local_haptics().provides(Arc::clone(&haptics)),
                ],
                || {
                    crate::input_method::provide(
                        InputConnection::new(field.editor, field.session),
                        content,
                    );
                },
            );
        }
    }
}

// The GPU the board draws with, made once per process.
fn new_gpu<W: KeyboardWindow>(
    settings: &AppSettings,
) -> Result<GpuContext, KeyboardError<W::Error>> {
    let instance = W::instance(settings);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))?;
    let (device, queue) = W::device(&adapter).map_err(KeyboardError::Platform)?;
    Ok(GpuContext {
        instance,
        adapter,
        device: Arc::new(device),
        queue: Arc::new(queue),
    })
}

impl<W> Host<W> {
    fn show(&mut self, size: (u32, u32), density: f32) -> Result<(), GuestViewError> {
        self.view.resize(size.0, size.1, density)?;
        self.view.set_visible(true);
        self.density = density;
        Ok(())
    }
}

// Draws the board when it changed. Returns whether it still owes a frame,
// because the surface gave no image this time.
fn draw<W>(view: &mut GuestView, target: &mut Target<W>, gpu: &GpuContext) -> bool {
    if !view.update() {
        return false;
    }
    let frame = match crate::wgpu_surface::current_surface_texture(&target.surface, "keyboard") {
        SurfaceFrame::Ready(frame) => frame,
        SurfaceFrame::Reconfigure => {
            target.surface.configure(&gpu.device, &target.config);
            return true;
        }
        SurfaceFrame::Skip => return true,
    };
    if let Err(error) = view.draw_surface(frame) {
        log::error!("the keyboard did not draw: {error}");
    }
    false
}
