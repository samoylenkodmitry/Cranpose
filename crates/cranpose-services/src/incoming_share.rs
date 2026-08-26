//! Content another application shares into this one.
//!
//! A share, a document intent, an "open with", a dropped file: all of them
//! arrive as [`IncomingContent`] and are collected as a stream scoped to the
//! composition. Nothing polls and nothing drains a queue — an item published
//! before any screen is listening waits in the framework's own backlog and is
//! handed to the first collector.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use cranpose_core::{EventStream, rememberEventStream};

use crate::content::{BytesContent, ContentHandle, ContentMetadata, resolve_content};

/// Where the bytes of an incoming item live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncomingSource {
    /// The platform handed over the bytes directly.
    Bytes(Vec<u8>),
    /// The platform named the content; it is opened through the content
    /// resolver when it is read.
    Uri(String),
}

/// One item shared into the application.
///
/// This is `Send` on purpose: platform hosts publish from whatever thread
/// received the intent, and the framework hops it onto the UI thread before the
/// composition turns it into a [`ContentHandle`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingContent {
    /// Display name, when the sender provided one.
    pub name: Option<String>,
    /// MIME type, when the sender provided one.
    pub mime_type: Option<String>,
    /// Where the bytes are.
    pub source: IncomingSource,
}

impl IncomingContent {
    /// An item whose bytes the platform already has.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            name: None,
            mime_type: None,
            source: IncomingSource::Bytes(bytes),
        }
    }

    /// An item the platform named rather than materialised.
    pub fn from_uri(uri: impl Into<String>) -> Self {
        Self {
            name: None,
            mime_type: None,
            source: IncomingSource::Uri(uri.into()),
        }
    }

    /// Sets the display name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the MIME type.
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// The display name the platform reported, or the last URI segment.
    pub fn display_name(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        match &self.source {
            IncomingSource::Bytes(_) => "shared".to_string(),
            IncomingSource::Uri(uri) => uri
                .rsplit(['/', ':'])
                .find(|segment| !segment.is_empty())
                .unwrap_or(uri)
                .to_string(),
        }
    }

    /// Turns this item into readable content. Call it on the UI thread, where
    /// the platform's content resolver lives.
    ///
    /// Returns `None` only when the item names a URI this platform cannot open.
    pub fn content(self) -> Option<ContentHandle> {
        let name = self.display_name();
        match self.source {
            IncomingSource::Bytes(bytes) => {
                let mut metadata = ContentMetadata::named(name);
                metadata.mime_type = self.mime_type;
                Some(BytesContent::new(metadata, bytes).handle())
            }
            IncomingSource::Uri(uri) => resolve_content(&uri),
        }
    }
}

type Observer = Arc<dyn Fn(IncomingContent) + Send + Sync>;

struct Inbox {
    observers: Vec<(u64, Observer)>,
    /// Items published before any composition was collecting. They are handed
    /// to the first observer that registers, so a share that launched the app
    /// is never lost.
    backlog: VecDeque<IncomingContent>,
}

impl Inbox {
    fn new() -> Self {
        Self {
            observers: Vec::new(),
            backlog: VecDeque::new(),
        }
    }

    /// Registers `observer` and returns the backlog it has to replay.
    ///
    /// The replay is handed back rather than delivered here so the caller can
    /// leave the lock before running application code.
    fn observe(&mut self, id: u64, observer: Observer) -> Vec<IncomingContent> {
        self.observers.push((id, observer));
        self.backlog.drain(..).collect()
    }

    /// The observers `content` should reach, or `None` when it went to the
    /// backlog because nobody is listening yet.
    fn publish(&mut self, content: IncomingContent) -> Option<Vec<Observer>> {
        if self.observers.is_empty() {
            self.backlog.push_back(content);
            return None;
        }
        Some(
            self.observers
                .iter()
                .map(|(_, observer)| Arc::clone(observer))
                .collect(),
        )
    }

    fn remove_observer(&mut self, id: u64) {
        self.observers.retain(|(existing, _)| *existing != id);
    }

    fn clear(&mut self) {
        self.backlog.clear();
    }
}

fn inbox() -> &'static Mutex<Inbox> {
    static INBOX: OnceLock<Mutex<Inbox>> = OnceLock::new();
    INBOX.get_or_init(|| Mutex::new(Inbox::new()))
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Keeps an observer registered until it is dropped.
pub struct IncomingContentObserver {
    id: u64,
}

impl Drop for IncomingContentObserver {
    fn drop(&mut self) {
        if let Ok(mut inbox) = inbox().lock() {
            inbox.remove_observer(self.id);
        }
    }
}

/// Registers `observer` for incoming content, replaying anything that arrived
/// before there was anywhere to put it.
///
/// Applications collect the stream from
/// [`rememberIncomingContent`] instead of calling this.
pub fn observe_incoming_content(
    observer: impl Fn(IncomingContent) + Send + Sync + 'static,
) -> IncomingContentObserver {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let observer: Observer = Arc::new(observer);
    let replay = {
        let Ok(mut inbox) = inbox().lock() else {
            return IncomingContentObserver { id };
        };
        inbox.observe(id, Arc::clone(&observer))
    };
    for item in replay {
        observer(item);
    }
    IncomingContentObserver { id }
}

/// Publishes content shared into the application. Callable from any thread; the
/// framework moves each item onto the UI thread before a composition sees it.
pub fn publish_incoming_content(content: IncomingContent) {
    let Some(observers) = ({
        let Ok(mut inbox) = inbox().lock() else {
            return;
        };
        inbox.publish(content.clone())
    }) else {
        return;
    };
    // Every observer sees every item: two screens can each react to a share.
    for observer in observers {
        observer(content.clone());
    }
}

/// Discards anything waiting for a collector. Used by tests and host teardown.
pub fn clear_incoming_content() {
    if let Ok(mut inbox) = inbox().lock() {
        inbox.clear();
    }
}

/// Collects content shared into the application for as long as this call stays
/// in the composition.
///
/// ```rust,no_run
/// use cranpose_macros::composable;
/// use cranpose_services::rememberIncomingContent;
///
/// #[composable]
/// fn Inbox() {
///     let shared = rememberIncomingContent();
///     cranpose_core::CollectEvents(shared, (), |item| {
///         log::info!("received {}", item.display_name());
///     });
/// }
/// ```
#[allow(non_snake_case)]
pub fn rememberIncomingContent() -> EventStream<IncomingContent> {
    rememberEventStream((), |sender| {
        observe_incoming_content(move |content| sender.send(content))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording_observer() -> (Observer, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let observer: Observer = Arc::new(move |item: IncomingContent| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(item.display_name())
        });
        (observer, seen)
    }

    /// These exercise `Inbox` directly rather than the process-global one.
    ///
    /// The global is a single instance shared by every test in this binary, and
    /// the test harness runs tests on parallel threads: one test's publish and
    /// another's clear interleave, and the assertion fails for reasons that
    /// have nothing to do with the code under test. Clearing at the start and
    /// end of each test does not fix that, it only narrows the window. The
    /// behaviour worth pinning lives in `Inbox`, which a test can own outright.
    #[test]
    fn an_item_published_before_anyone_listens_is_backlogged_once() {
        let mut inbox = Inbox::new();

        assert!(
            inbox
                .publish(IncomingContent::from_bytes(vec![1, 2, 3]).with_name("scan.jpg"))
                .is_none(),
            "with nobody listening the item belongs in the backlog, not delivered"
        );

        let (first, _first_seen) = recording_observer();
        assert_eq!(
            inbox.observe(1, first).len(),
            1,
            "the first observer to register receives the backlog"
        );

        let (second, _second_seen) = recording_observer();
        assert!(
            inbox.observe(2, second).is_empty(),
            "the backlog drains into that observer and is not replayed to the next"
        );
    }

    #[test]
    fn the_backlog_reaches_the_first_observer_that_registers() {
        let mut inbox = Inbox::new();
        inbox.publish(IncomingContent::from_bytes(vec![1, 2, 3]).with_name("scan.jpg"));

        let (observer, seen) = recording_observer();
        let replay = inbox.observe(1, Arc::clone(&observer));
        for item in replay {
            observer(item);
        }

        assert_eq!(
            seen.lock().unwrap_or_else(|e| e.into_inner()).as_slice(),
            ["scan.jpg"]
        );
    }

    #[test]
    fn observers_stop_receiving_once_dropped() {
        let mut inbox = Inbox::new();
        let (observer, _seen) = recording_observer();
        inbox.observe(7, observer);

        let delivered = inbox
            .publish(IncomingContent::from_bytes(vec![1]))
            .expect("a registered observer receives the item");
        assert_eq!(delivered.len(), 1);

        inbox.remove_observer(7);
        assert!(
            inbox
                .publish(IncomingContent::from_bytes(vec![2]))
                .is_none(),
            "once the last observer is gone the item goes back to the backlog"
        );
    }

    #[test]
    fn clearing_discards_the_backlog() {
        let mut inbox = Inbox::new();
        inbox.publish(IncomingContent::from_bytes(vec![1]));
        inbox.clear();

        let (observer, _seen) = recording_observer();
        assert!(
            inbox.observe(1, observer).is_empty(),
            "a cleared backlog has nothing left to replay"
        );
    }

    #[test]
    fn bytes_become_readable_content_with_the_reported_name() {
        let item = IncomingContent::from_bytes(b"payload".to_vec())
            .with_name("note.txt")
            .with_mime_type("text/plain");
        let content = item.content().expect("bytes always resolve");
        assert_eq!(content.metadata().name, "note.txt");
        assert_eq!(content.metadata().mime_type.as_deref(), Some("text/plain"));
        assert_eq!(
            pollster::block_on(content.read_all()).expect("the bytes read back"),
            b"payload"
        );
    }

    #[test]
    fn a_uri_item_names_itself_from_its_last_segment() {
        let item = IncomingContent::from_uri("content://media/external/images/42");
        assert_eq!(item.display_name(), "42");
    }
}
