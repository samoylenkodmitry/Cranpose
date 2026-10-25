#![expect(unsafe_code)]

//! The link between an iPhone app and its watch app, through WatchConnectivity.
//!
//! WatchConnectivity carries messages and no streams, so the link is a
//! [`FramedWearableLink`]: every frame is one message data of the session. The
//! session has one counterpart, the paired watch on the phone and the phone on
//! the watch, and its peer id is always [`PEER`].

use std::{
    ptr::NonNull,
    sync::{Arc, Condvar, Mutex, Once, OnceLock, PoisonError, mpsc::sync_channel},
    time::Duration,
};

use block2::{DynBlock, RcBlock};
use cranpose_services::{
    FramedWearableLink, WearableError, WearablePeer, WearableTransport, set_platform_wearable_link,
};
use objc2::{AnyThread, define_class, msg_send, rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{NSData, NSError, NSObject, NSObjectProtocol};
use objc2_watch_connectivity::{WCSession, WCSessionActivationState, WCSessionDelegate};

/// The one device at the other end of the session.
const PEER: &str = "paired";

/// How long a frame that needs an answer waits for the other application.
const REPLY_WAIT: Duration = Duration::from_secs(10);

/// How long a call waits for the session to start and for the other
/// application to come in reach: both take a moment after either launches.
const STATE_WAIT: Duration = Duration::from_secs(3);

static LINK: OnceLock<Arc<FramedWearableLink<WatchConnectivity>>> = OnceLock::new();

/// Taken by the delegate to announce a change of the session's state, so a
/// caller that checked the state under it cannot miss the announcement.
static STATE_LOCK: Mutex<()> = Mutex::new(());
static STATE_CHANGED: Condvar = Condvar::new();

pub(crate) fn register() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        // SAFETY: a class method without arguments.
        if !unsafe { WCSession::isSupported() } {
            return;
        }
        let link =
            LINK.get_or_init(|| Arc::new(FramedWearableLink::new(WatchConnectivity::default())));
        set_platform_wearable_link(Some(Arc::clone(link) as _));
        let delegate = SessionDelegate::new();
        // SAFETY: the default session is valid for the life of the process, and
        // the delegate answers every method the protocol requires.
        unsafe {
            let session = WCSession::defaultSession();
            session.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            session.activateSession();
        }
        // The session holds its delegate weakly; this one serves the process.
        std::mem::forget(delegate);
    });
}

/// The session once `ready` holds for it, waiting up to [`STATE_WAIT`].
fn session_when(ready: fn(&WCSession) -> bool) -> Option<Retained<WCSession>> {
    // SAFETY: the default session is valid for the life of the process.
    let session = unsafe { WCSession::defaultSession() };
    let checked = STATE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let (_checked, waited) = STATE_CHANGED
        .wait_timeout_while(checked, STATE_WAIT, |()| !ready(&session))
        .unwrap_or_else(PoisonError::into_inner);
    (!waited.timed_out()).then_some(session)
}

fn active(session: &WCSession) -> bool {
    // SAFETY: reading the session's state.
    unsafe { session.activationState() == WCSessionActivationState::Activated }
}

fn in_reach(session: &WCSession) -> bool {
    // SAFETY: reading the session's state.
    active(session) && unsafe { session.isReachable() }
}

fn announce_state() {
    let _announcing = STATE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    STATE_CHANGED.notify_all();
}

fn describe(error: NonNull<NSError>) -> String {
    // SAFETY: WatchConnectivity hands a valid error to the handler.
    unsafe { error.as_ref() }.localizedDescription().to_string()
}

#[derive(Default)]
struct WatchConnectivity {
    /// Why an earlier frame sent without an answer failed; the next send
    /// reports it.
    failure: Arc<Mutex<Option<String>>>,
}

impl WearableTransport for WatchConnectivity {
    fn peers(&self) -> Result<Vec<WearablePeer>, WearableError> {
        let Some(session) = session_when(active) else {
            return Ok(Vec::new());
        };
        // SAFETY: reading the session's state; each property exists on the
        // platform that reads it.
        #[cfg(target_os = "ios")]
        let (present, name, has_app) = unsafe {
            (
                session.isPaired(),
                "Apple Watch",
                session.isWatchAppInstalled(),
            )
        };
        #[cfg(target_os = "watchos")]
        let (present, name, has_app) =
            (true, "iPhone", unsafe { session.isCompanionAppInstalled() });
        Ok(present
            .then(|| WearablePeer {
                id: PEER.to_owned(),
                name: name.to_owned(),
                has_app,
            })
            .into_iter()
            .collect())
    }

    fn send(&self, _peer: &str, frame: &[u8], confirm: bool) -> Result<(), WearableError> {
        let session = session_when(in_reach).ok_or_else(|| {
            WearableError::Failed("the app on the paired device is out of reach".into())
        })?;
        let data = NSData::with_bytes(frame);
        if confirm {
            let (answer, answered) = sync_channel::<Result<(), String>>(1);
            let failed = answer.clone();
            let reply = RcBlock::new(move |_: NonNull<NSData>| {
                let _ = answer.try_send(Ok(()));
            });
            let error = RcBlock::new(move |error: NonNull<NSError>| {
                let _ = failed.try_send(Err(describe(error)));
            });
            // SAFETY: WatchConnectivity copies the blocks and calls one of them
            // once, on its own queue.
            unsafe {
                session.sendMessageData_replyHandler_errorHandler(
                    &data,
                    Some(&reply),
                    Some(&error),
                );
            }
            return match answered.recv_timeout(REPLY_WAIT) {
                Ok(result) => result.map_err(WearableError::Failed),
                Err(_) => Err(WearableError::Failed(
                    "the app on the paired device did not answer".into(),
                )),
            };
        }
        if let Some(earlier) = self
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            return Err(WearableError::Failed(earlier));
        }
        let failure = Arc::clone(&self.failure);
        let error = RcBlock::new(move |error: NonNull<NSError>| {
            *failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(describe(error));
        });
        // SAFETY: as above; without a reply handler only a failure calls back.
        unsafe {
            session.sendMessageData_replyHandler_errorHandler(&data, None, Some(&error));
        }
        Ok(())
    }

    fn open_url(&self, _peer: &str, _url: &str) -> Result<(), WearableError> {
        Err(WearableError::Failed(
            "an iPhone and its watch cannot open links on each other".into(),
        ))
    }
}

fn receive(data: &NSData) {
    if let Some(link) = LINK.get() {
        // SAFETY: nothing changes the data while the link reads it.
        link.receive(PEER, unsafe { data.as_bytes_unchecked() });
    }
}

fn lose_streams() {
    if let Some(link) = LINK.get() {
        link.disconnect(PEER);
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the delegate keeps
    // no state of its own.
    #[unsafe(super(NSObject))]
    #[name = "CranposeWatchConnectivityDelegate"]
    struct SessionDelegate;

    unsafe impl NSObjectProtocol for SessionDelegate {}

    // WatchConnectivity calls these on its own serial queue.
    unsafe impl WCSessionDelegate for SessionDelegate {
        #[unsafe(method(session:activationDidCompleteWithState:error:))]
        fn activated(
            &self,
            _session: &WCSession,
            _state: WCSessionActivationState,
            error: Option<&NSError>,
        ) {
            if let Some(error) = error {
                log::warn!(
                    "wearable: the session did not start: {}",
                    error.localizedDescription()
                );
            }
            announce_state();
        }

        #[unsafe(method(sessionReachabilityDidChange:))]
        fn reach_changed(&self, _session: &WCSession) {
            announce_state();
        }

        #[unsafe(method(sessionDidBecomeInactive:))]
        fn became_inactive(&self, _session: &WCSession) {
            lose_streams();
        }

        /// The person chose another watch; the session starts again for it.
        #[unsafe(method(sessionDidDeactivate:))]
        fn deactivated(&self, session: &WCSession) {
            // SAFETY: starting the session again, as the protocol asks here.
            unsafe { session.activateSession() };
        }

        #[cfg(target_os = "ios")]
        #[unsafe(method(sessionWatchStateDidChange:))]
        fn watch_changed(&self, session: &WCSession) {
            // SAFETY: reading the session's state.
            if !unsafe { session.isPaired() && session.isWatchAppInstalled() } {
                lose_streams();
            }
            announce_state();
        }

        #[unsafe(method(session:didReceiveMessageData:))]
        fn received(&self, _session: &WCSession, data: &NSData) {
            receive(data);
        }

        #[unsafe(method(session:didReceiveMessageData:replyHandler:))]
        fn received_with_reply(
            &self,
            _session: &WCSession,
            data: &NSData,
            reply: &DynBlock<dyn Fn(NonNull<NSData>)>,
        ) {
            receive(data);
            reply.call((NonNull::from(&*NSData::new()),));
        }
    }
);

impl SessionDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}
