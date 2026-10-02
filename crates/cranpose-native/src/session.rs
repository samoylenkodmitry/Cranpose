use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use tokio::sync::oneshot;

use crate::{NativeContent, worker};

/// A native hosting failure.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum NativeError {
    /// The component has shut down.
    #[error("component is closed")]
    Closed,
    /// Rendering or platform integration failed.
    #[error("{detail}")]
    Runtime {
        /// Description of the failure.
        detail: String,
    },
}

/// One application event exchanged with the native host.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NativeEvent {
    /// Application-defined event name.
    pub name: String,
    /// Application-defined event value.
    pub value: String,
}

/// Touch input in logical host coordinates.
#[derive(Clone, Copy, uniffi::Enum)]
pub enum TouchPhase {
    /// Begins a gesture.
    Down,
    /// Moves the active pointer.
    Move,
    /// Ends the gesture.
    Up,
    /// Cancels the gesture.
    Cancel,
}

/// One mounted native child.
#[derive(uniffi::Record)]
pub struct NativeSlot {
    /// Stable identity within the session.
    pub id: u64,
    /// Registered platform factory name.
    pub kind: String,
    /// Factory configuration.
    pub value: String,
    /// Logical horizontal offset.
    pub x: f32,
    /// Logical vertical offset.
    pub y: f32,
    /// Logical width.
    pub width: f32,
    /// Logical height.
    pub height: f32,
}

/// A presentation update. Empty pixels preserve the currently presented image and slots.
#[derive(uniffi::Record)]
pub struct NativeFrame {
    /// Physical image width.
    pub width: u32,
    /// Physical image height.
    pub height: u32,
    /// Premultiplied RGBA pixels, empty when unchanged.
    pub pixels: Vec<u8>,
    /// Complete native-child snapshot accompanying a changed image.
    pub slots: Vec<NativeSlot>,
    /// Application events emitted since the preceding frame.
    pub events: Vec<NativeEvent>,
    /// Delay until the next scheduled update, absent when idle.
    pub next_frame_ms: Option<u64>,
}

/// Wakes a platform view when its composition needs a frame.
#[uniffi::export(callback_interface)]
pub trait FrameListener: Send + Sync {
    /// Schedule a frame on the platform UI thread.
    fn request_frame(&self);
}

pub(crate) enum Command {
    Frame(
        u32,
        u32,
        f32,
        oneshot::Sender<Result<NativeFrame, NativeError>>,
    ),
    Touch(TouchPhase, f32, f32),
    Event(NativeEvent),
    NativeEvent(u64, String),
    Visible(bool),
    Close,
}

#[derive(Default)]
pub(crate) struct Wake(Mutex<Option<Arc<dyn FrameListener>>>);

impl Wake {
    pub(crate) fn request(&self) {
        let listener = self
            .0
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(Arc::clone));
        if let Some(listener) = listener {
            listener.request_frame();
        }
    }
}

/// Owns an embedded composition and its worker thread.
///
/// Platform `CranposeView` adapters drive this session automatically. An application
/// creates one with `NativeSession::new` and supplies only its content and events.
#[derive(uniffi::Object)]
pub struct NativeSession {
    sender: mpsc::Sender<Command>,
    closed: AtomicBool,
    wake: Arc<Wake>,
}

impl NativeSession {
    /// Constructs content on the owning worker. Captures crossing into this factory
    /// must be Send; the composition and its state stay on that worker afterwards.
    pub fn new(factory: impl FnOnce() -> NativeContent + Send + 'static) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel();
        let wake = Arc::new(Wake::default());
        let worker_wake = Arc::clone(&wake);
        std::thread::spawn(move || worker::run(receiver, factory, worker_wake));
        Arc::new(Self {
            sender,
            closed: AtomicBool::new(false),
            wake,
        })
    }

    fn send(&self, command: Command) -> Result<(), NativeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(NativeError::Closed);
        }
        self.sender.send(command).map_err(|_| NativeError::Closed)
    }
}

#[uniffi::export]
impl NativeSession {
    /// Attaches the platform frame callback and requests an initial frame.
    pub fn set_listener(&self, listener: Box<dyn FrameListener>) -> Result<(), NativeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(NativeError::Closed);
        }
        *self.wake.0.lock().map_err(|_| NativeError::Closed)? = Some(Arc::from(listener));
        self.wake.request();
        Ok(())
    }

    /// Produces the next presentation update. Platform adapters serialize these calls.
    pub async fn frame(
        &self,
        width: u32,
        height: u32,
        density: f32,
    ) -> Result<NativeFrame, NativeError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Frame(width, height, density, sender))?;
        receiver.await.map_err(|_| NativeError::Closed)?
    }

    /// Delivers application input without exposing rendering internals.
    pub fn send_event(&self, name: String, value: String) -> Result<(), NativeError> {
        self.send(Command::Event(NativeEvent { name, value }))?;
        self.wake.request();
        Ok(())
    }

    /// Routes a pointer event in logical points.
    pub fn touch(&self, phase: TouchPhase, x: f32, y: f32) -> Result<(), NativeError> {
        self.send(Command::Touch(phase, x, y))?;
        self.wake.request();
        Ok(())
    }

    /// Routes a factory event from a mounted native child; stale IDs are ignored.
    pub fn native_event(&self, id: u64, event: String) -> Result<(), NativeError> {
        self.send(Command::NativeEvent(id, event))?;
        self.wake.request();
        Ok(())
    }

    /// Suspends hidden or detached content and resumes it when visible.
    pub fn set_visible(&self, visible: bool) -> Result<(), NativeError> {
        self.send(Command::Visible(visible))?;
        if visible {
            self.wake.request();
        }
        Ok(())
    }

    /// Releases the worker and its composition. Safe to call repeatedly.
    pub fn shutdown(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            if let Ok(mut listener) = self.wake.0.lock() {
                *listener = None;
            }
            let _ = self.sender.send(Command::Close);
        }
    }
}

impl Drop for NativeSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}
