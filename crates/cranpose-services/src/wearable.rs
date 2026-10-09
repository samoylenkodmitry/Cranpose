//! Messages and byte streams between an application and the same application
//! on the paired phone or watch: a watch app streams its microphone to the
//! phone, and the phone sends its results back.
//!
//! On Android the link is the Wear OS Data Layer. An application turns it on
//! with `cranpose { services.add("wearable") }`, which brings the framework's
//! Java and Google Play services' wearable library into its build. On other
//! platforms, and in a build without the service, every call returns
//! [`WearableError::Unavailable`].
//!
//! **Sending.** [`wearable_peers`] lists the connected devices,
//! [`send_wearable_message`] delivers a small message, and
//! [`open_wearable_stream`] opens a byte stream for continuous data such as
//! audio. Each call blocks until the platform answers, so call them off the
//! UI thread.
//!
//! **Receiving.** The application installs one [`WearableReceiver`] with
//! [`set_wearable_receiver`]. Events arrive on a platform thread. A
//! [`WearableStream`] is read until it ends, on whatever thread the receiver
//! hands it to. An event that arrives while no receiver is installed is
//! dropped.

use std::{
    io::{Read, Write},
    sync::Arc,
};

use crate::registry::ServiceRegistry;

/// A connected phone or watch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WearablePeer {
    /// The platform's identifier of the device, the `peer` the other calls take.
    pub id: String,
    /// The name the person gave the device.
    pub name: String,
}

/// Why a call to the paired device did not happen.
#[derive(thiserror::Error, Debug)]
pub enum WearableError {
    /// This platform or this build has no link to a paired device.
    #[error("there is no link to a paired device in this build")]
    Unavailable,
    /// The platform reported a failure, for example a device out of range.
    #[error("the paired device link failed: {0}")]
    Failed(String),
}

/// A byte stream the paired device opened to this application.
pub struct WearableStream {
    /// The device that opened it.
    pub peer: String,
    /// The path it was opened under.
    pub path: String,
    reader: Box<dyn Read + Send>,
}

impl WearableStream {
    /// A stream from `peer` under `path`, read through `reader`. Platform
    /// backends create it.
    pub fn new(peer: String, path: String, reader: Box<dyn Read + Send>) -> Self {
        Self { peer, path, reader }
    }
}

impl Read for WearableStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}

/// Something the paired device sent.
pub enum WearableEvent {
    /// A message sent with [`send_wearable_message`] on the other device.
    Message {
        /// The device that sent it.
        peer: String,
        /// The path it was sent under.
        path: String,
        /// Its bytes.
        data: Vec<u8>,
    },
    /// A stream opened with [`open_wearable_stream`] on the other device.
    Stream(WearableStream),
}

/// Receives what the paired device sends.
pub trait WearableReceiver: Send + Sync {
    /// Handles one event, on a platform thread.
    fn receive(&self, event: WearableEvent);
}

impl<F: Fn(WearableEvent) + Send + Sync> WearableReceiver for F {
    fn receive(&self, event: WearableEvent) {
        self(event);
    }
}

/// The platform's link to the paired device, installed by the platform
/// backend with [`set_platform_wearable_link`].
pub trait WearableLink: Send + Sync {
    /// The connected devices.
    fn peers(&self) -> Result<Vec<WearablePeer>, WearableError>;
    /// Delivers `data` to `peer` under `path`.
    fn send_message(&self, peer: &str, path: &str, data: &[u8]) -> Result<(), WearableError>;
    /// Opens a byte stream to `peer` under `path`.
    fn open_stream(&self, peer: &str, path: &str) -> Result<Box<dyn Write + Send>, WearableError>;
}

static PLATFORM: ServiceRegistry<dyn WearableLink> = ServiceRegistry::new();
static RECEIVER: ServiceRegistry<dyn WearableReceiver> = ServiceRegistry::new();

/// Installs the platform's link. Platform backends call it.
pub fn set_platform_wearable_link(link: Arc<dyn WearableLink>) {
    PLATFORM.set(link);
}

/// Removes the platform's link; every call then returns
/// [`WearableError::Unavailable`].
pub fn clear_platform_wearable_link() {
    PLATFORM.clear();
}

fn link() -> Result<Arc<dyn WearableLink>, WearableError> {
    PLATFORM.get().ok_or(WearableError::Unavailable)
}

/// The connected phones or watches.
pub fn wearable_peers() -> Result<Vec<WearablePeer>, WearableError> {
    link()?.peers()
}

/// Delivers `data` to the application on `peer` under `path`.
pub fn send_wearable_message(peer: &str, path: &str, data: &[u8]) -> Result<(), WearableError> {
    link()?.send_message(peer, path, data)
}

/// Opens a byte stream to the application on `peer` under `path`; the other
/// device receives it as [`WearableEvent::Stream`]. Dropping the writer
/// closes the stream.
pub fn open_wearable_stream(
    peer: &str,
    path: &str,
) -> Result<Box<dyn Write + Send>, WearableError> {
    link()?.open_stream(peer, path)
}

/// Installs the application's receiver of what the paired device sends,
/// replacing any earlier one.
pub fn set_wearable_receiver(receiver: Arc<dyn WearableReceiver>) {
    RECEIVER.set(receiver);
}

/// Removes the application's receiver; later events are dropped.
pub fn clear_wearable_receiver() {
    RECEIVER.clear();
}

/// Hands an event from the platform to the application's receiver, or drops
/// it when there is none. Platform backends call it.
pub fn publish_wearable_event(event: WearableEvent) {
    match RECEIVER.get() {
        Some(receiver) => receiver.receive(event),
        None => log::warn!(
            "a paired device sent something, and the application has no wearable receiver"
        ),
    }
}

#[cfg(test)]
#[path = "tests/wearable_tests.rs"]
mod tests;
