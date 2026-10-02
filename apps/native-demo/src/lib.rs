#![deny(unsafe_code)]

mod screen;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use cranpose::{AppLauncher, embedded_view::EmbeddedView, native_view::NativeViewHost};
use tokio::sync::oneshot;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum DemoError {
    #[error("component is closed")]
    Closed,
    #[error("{detail}")]
    Runtime { detail: String },
}

#[derive(Clone, Copy, uniffi::Enum)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(uniffi::Record)]
pub struct NativeSlot {
    pub id: u64,
    pub kind: String,
    pub value: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(uniffi::Record)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub slots: Vec<NativeSlot>,
    pub count: i32,
    pub next_frame_ms: Option<u64>,
}

#[uniffi::export(callback_interface)]
pub trait FrameListener: Send + Sync {
    fn request_frame(&self);
}

enum Command {
    Frame(u32, u32, f32, oneshot::Sender<Result<Frame, DemoError>>),
    Touch(TouchPhase, f32, f32),
    Increment,
    NativeEvent(u64, String),
    Visible(bool),
    Close,
}

#[derive(uniffi::Object)]
pub struct DemoSession {
    sender: mpsc::Sender<Command>,
    closed: AtomicBool,
}

#[uniffi::export]
impl DemoSession {
    #[uniffi::constructor]
    pub fn new(native_children: bool, listener: Box<dyn FrameListener>) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || run(receiver, native_children, listener));
        Arc::new(Self {
            sender,
            closed: AtomicBool::new(false),
        })
    }

    pub async fn frame(&self, width: u32, height: u32, density: f32) -> Result<Frame, DemoError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Frame(width, height, density, sender))?;
        receiver.await.map_err(|_| DemoError::Closed)?
    }

    pub fn touch(&self, phase: TouchPhase, x: f32, y: f32) -> Result<(), DemoError> {
        self.send(Command::Touch(phase, x, y))
    }

    pub fn increment(&self) -> Result<(), DemoError> {
        self.send(Command::Increment)
    }

    pub fn native_event(&self, id: u64, event: String) -> Result<(), DemoError> {
        self.send(Command::NativeEvent(id, event))
    }

    pub fn set_visible(&self, visible: bool) -> Result<(), DemoError> {
        self.send(Command::Visible(visible))
    }

    pub fn shutdown(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let _ = self.sender.send(Command::Close);
        }
    }
}

impl DemoSession {
    fn send(&self, command: Command) -> Result<(), DemoError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DemoError::Closed);
        }
        self.sender.send(command).map_err(|_| DemoError::Closed)
    }
}

impl Drop for DemoSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Component {
    view: EmbeddedView,
    native: NativeViewHost,
    state: screen::DemoState,
}

impl Component {
    fn new(native_children: bool, listener: Arc<dyn FrameListener>) -> Result<Self, DemoError> {
        let native = NativeViewHost::default();
        let state = screen::DemoState::default();
        let content_state = state.clone();
        let content_host = native.clone();
        let mut view = AppLauncher::new()
            .create_embedded_view(1, 1, 1.0, move || {
                screen::content(content_host.clone(), content_state.clone(), native_children);
            })
            .map_err(runtime_error)?;
        view.shell()
            .set_frame_waker(move || listener.request_frame());
        Ok(Self {
            view,
            native,
            state,
        })
    }

    fn frame(&mut self, width: u32, height: u32, density: f32) -> Result<Frame, DemoError> {
        self.view
            .resize(width, height, density)
            .map_err(runtime_error)?;
        let mut pixels = Vec::new();
        let changed = self.view.draw(&mut pixels).map_err(runtime_error)?;
        let slots = if changed && !self.native.is_empty() {
            self.native
                .layout(self.view.shell().layout_tree())
                .into_iter()
                .map(|slot| NativeSlot {
                    id: slot.id,
                    kind: slot.kind,
                    value: slot.value,
                    x: slot.bounds.x,
                    y: slot.bounds.y,
                    width: slot.bounds.width,
                    height: slot.bounds.height,
                })
                .collect()
        } else {
            Vec::new()
        };
        let schedule = self.view.shell().frame_schedule();
        let next_frame_ms = if schedule.needs_frame || schedule.needs_update {
            Some(16)
        } else {
            schedule.next_deadline.map(|deadline| {
                deadline
                    .saturating_duration_since(web_time::Instant::now())
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            })
        };
        Ok(Frame {
            width,
            height,
            pixels,
            slots,
            count: self.state.count(),
            next_frame_ms,
        })
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Frame(width, height, density, reply) => {
                let _ = reply.send(self.frame(width, height, density));
            }
            Command::Touch(phase, x, y) => {
                let shell = self.view.shell();
                shell.set_pointer_source(cranpose::prelude::PointerSource::Touch);
                match phase {
                    TouchPhase::Down => {
                        shell.set_cursor(x, y);
                        shell.pointer_pressed();
                    }
                    TouchPhase::Move => {
                        shell.set_cursor(x, y);
                    }
                    TouchPhase::Up => {
                        shell.pointer_released_at_position(x, y);
                    }
                    TouchPhase::Cancel => shell.cancel_gesture(),
                }
            }
            Command::Increment => self.state.increment(),
            Command::NativeEvent(id, event) => {
                self.native.dispatch(id, &event);
            }
            Command::Visible(visible) => self.view.set_visible(visible),
            Command::Close => {}
        }
    }
}

fn runtime_error(error: impl std::fmt::Display) -> DemoError {
    DemoError::Runtime {
        detail: error.to_string(),
    }
}

fn run(receiver: mpsc::Receiver<Command>, native_children: bool, listener: Box<dyn FrameListener>) {
    let listener: Arc<dyn FrameListener> = Arc::from(listener);
    let mut component = Component::new(native_children, listener);
    while let Ok(command) = receiver.recv() {
        if matches!(command, Command::Close) {
            break;
        }
        match &mut component {
            Ok(component) => component.handle(command),
            Err(error) => {
                if let Command::Frame(_, _, _, reply) = command {
                    let _ = reply.send(Err(runtime_error(&error)));
                }
            }
        }
    }
}
