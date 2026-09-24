//! Messages between the application and the program embedding it.
//!
//! An application launched inside another program — an IDE tool window, an
//! editor panel — shares a message line with that host. Every message names a
//! channel and carries a text payload; which channels exist and what their
//! payloads hold is for the host and the application to agree on (JSON is the
//! usual choice). The newest message on each channel is kept, so a screen that
//! starts collecting after the host spoke still learns the current value.
//!
//! Outside a host nothing is ever received and [`send_to_host`] reports that
//! no one listened, so the same application runs standalone unchanged.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use cranpose_core::{EventStream, rememberEventStream};

/// One message on the line between the application and its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostMessage {
    /// The channel the message belongs to.
    pub channel: String,
    /// The message body.
    pub payload: String,
}

impl HostMessage {
    /// A message carrying `payload` on `channel`.
    pub fn new(channel: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            payload: payload.into(),
        }
    }
}

type Observer = Arc<dyn Fn(String) + Send + Sync>;
type Outbox = Arc<dyn Fn(HostMessage) + Send + Sync>;

struct Mailbox {
    observers: Vec<(u64, String, Observer)>,
    latest: HashMap<String, String>,
}

impl Mailbox {
    fn new() -> Self {
        Self {
            observers: Vec::new(),
            latest: HashMap::new(),
        }
    }

    fn observe(&mut self, id: u64, channel: &str, observer: Observer) -> Option<String> {
        self.observers.push((id, channel.to_owned(), observer));
        self.latest.get(channel).cloned()
    }

    fn publish(&mut self, message: &HostMessage) -> Vec<Observer> {
        self.latest
            .insert(message.channel.clone(), message.payload.clone());
        self.observers
            .iter()
            .filter(|(_, channel, _)| *channel == message.channel)
            .map(|(_, _, observer)| Arc::clone(observer))
            .collect()
    }

    fn remove_observer(&mut self, id: u64) {
        self.observers.retain(|(existing, _, _)| *existing != id);
    }

    fn clear(&mut self) {
        self.latest.clear();
    }
}

fn mailbox() -> MutexGuard<'static, Mailbox> {
    static MAILBOX: OnceLock<Mutex<Mailbox>> = OnceLock::new();
    MAILBOX
        .get_or_init(|| Mutex::new(Mailbox::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn outbox() -> MutexGuard<'static, Option<Outbox>> {
    static OUTBOX: OnceLock<Mutex<Option<Outbox>>> = OnceLock::new();
    OUTBOX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Keeps a host message observer registered until it is dropped.
pub struct HostMessageObserver {
    id: u64,
}

impl Drop for HostMessageObserver {
    fn drop(&mut self) {
        mailbox().remove_observer(self.id);
    }
}

/// Registers `observer` for the messages the host sends on `channel`, handing
/// it the newest one at once when the host already spoke there.
///
/// Applications collect [`rememberHostMessages`] instead of calling this.
pub fn observe_host_messages(
    channel: &str,
    observer: impl Fn(String) + Send + Sync + 'static,
) -> HostMessageObserver {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let observer: Observer = Arc::new(observer);
    let replay = mailbox().observe(id, channel, Arc::clone(&observer));
    if let Some(payload) = replay {
        observer(payload);
    }
    HostMessageObserver { id }
}

/// Delivers a message the host sent to every observer of its channel.
/// Callable from any thread; platform hosts call it as messages arrive.
pub fn publish_host_message(message: HostMessage) {
    let observers = mailbox().publish(&message);
    for observer in observers {
        observer(message.payload.clone());
    }
}

/// Forgets the newest message of every channel. Used by tests and host
/// teardown.
pub fn clear_host_messages() {
    mailbox().clear();
}

/// Installs where [`send_to_host`] delivers: the platform host's connection.
pub fn install_host_outbox(outbox_fn: impl Fn(HostMessage) + Send + Sync + 'static) {
    *outbox() = Some(Arc::new(outbox_fn));
}

/// Removes the installed outbox, after which [`send_to_host`] reports that no
/// host listens.
pub fn clear_host_outbox() {
    *outbox() = None;
}

/// Sends `payload` on `channel` to the embedding host.
///
/// Returns `false` when the application runs without a host, so nothing was
/// sent. Callable from any thread.
pub fn send_to_host(channel: &str, payload: &str) -> bool {
    let Some(outbox_fn) = outbox().clone() else {
        log::debug!("host message on {channel} dropped: no host is attached");
        return false;
    };
    outbox_fn(HostMessage::new(channel, payload));
    true
}

/// Collects the messages the host sends on `channel` for as long as this call
/// stays in the composition, starting with the newest one already received.
///
/// ```rust,no_run
/// use cranpose_macros::composable;
/// use cranpose_services::rememberHostMessages;
///
/// #[composable]
/// fn CurrentFile() {
///     let file =
///         cranpose_core::collectAsState(rememberHostMessages("ide.editor"), (), String::new());
///     log::info!("the host shows {}", file.get());
/// }
/// ```
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberHostMessages(channel: &str) -> EventStream<String> {
    let channel = channel.to_owned();
    rememberEventStream(channel.clone(), move |sender| {
        observe_host_messages(&channel, move |payload| sender.send(payload))
    })
}

#[cfg(test)]
#[path = "tests/host_messages_tests.rs"]
mod tests;
