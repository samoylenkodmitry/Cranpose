//! Live document discovery and commands over the embedding host transport.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use cranpose_core::{CollectEvents, LaunchedEffect};
use cranpose_services::{rememberHostMessages, send_to_host};
use serde::{Deserialize, Serialize};

use crate::{Error, LiveView, Request, Session};

/// Host channel announcing a live document and its committed revision.
pub const READY_CHANNEL: &str = "cranpose.live.v1.ready";
/// Host channel accepting addressed, correlated runtime commands.
pub const REQUEST_CHANNEL: &str = "cranpose.live.v1.request";
/// Host channel carrying correlated command results.
pub const RESPONSE_CHANNEL: &str = "cranpose.live.v1.response";

#[derive(Serialize)]
struct Document {
    protocol: u8,
    document: String,
    file: PathBuf,
    function: String,
}

/// A live session addressable over Cranpose's existing embedding transport.
#[derive(Clone)]
pub struct HostedSession(Rc<HostedInner>);

struct HostedInner {
    session: Session,
    document: Document,
    last_reply: RefCell<Option<(u64, Rc<String>)>>,
    announced: Cell<bool>,
}

impl PartialEq for HostedSession {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    id: u64,
    document: String,
    request: serde_json::Value,
}

impl HostedSession {
    /// Associates a session with a dedicated, interpreted source file.
    ///
    /// The file must contain only interpreted UI: Studio excludes it from native
    /// compilation on save. Use [`original_source_path`] inside a private IDE build.
    /// The application must watch the file and apply saves to this session, as
    /// `cranpose-live-demo` does; hosting alone does not install a file watcher.
    pub fn new(session: Session, file: PathBuf, function: String) -> Self {
        Self(Rc::new(HostedInner {
            session,
            document: Document {
                protocol: 1,
                document: format!("{}#{function}", file.display()),
                file,
                function,
            },
            last_reply: RefCell::new(None),
            announced: Cell::new(false),
        }))
    }

    /// Serializes discovery metadata, including the current revision and limits.
    pub fn announcement(&self) -> Result<String, Error> {
        serde_json::to_string(&serde_json::json!({
            "protocol": 1,
            "document": self.0.document.document,
            "file": self.0.document.file,
            "function": self.0.document.function,
            "revision": self.0.session.revision(),
            "limits": self.0.session.registry().limits,
        }))
        .map_err(|error| Error::Invalid(error.to_string()))
    }

    /// Handles one addressed host command on the UI thread.
    ///
    /// Other documents return `None`. Rejected commands retain the last good UI
    /// and reply with their request ID, current revision and diagnostic. The host
    /// assigns increasing IDs per document. Replayed IDs never execute twice.
    pub fn handle_json(&self, payload: &str) -> Result<Option<Rc<String>>, Error> {
        let command: Command =
            serde_json::from_str(payload).map_err(|error| Error::Invalid(error.to_string()))?;
        if command.document != self.0.document.document {
            return Ok(None);
        }
        if let Some((id, reply)) = self.0.last_reply.borrow().as_ref() {
            if command.id == *id {
                return Ok(Some(Rc::clone(reply)));
            }
            if command.id < *id {
                return Err(Error::Invalid(
                    "host request ID precedes the last request".into(),
                ));
            }
        }
        #[derive(Serialize)]
        struct Reply<'a> {
            id: u64,
            document: &'a str,
            revision: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            response: Option<crate::Response<'a>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            error: Option<String>,
        }
        let result = serde_json::from_value::<Request>(command.request)
            .map_err(|error| Error::Invalid(error.to_string()))
            .and_then(|request| self.0.session.handle(request));
        let (response, error) = match result {
            Ok(response) => (Some(response), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let reply = Rc::new(
            serde_json::to_string(&Reply {
                id: command.id,
                document: &self.0.document.document,
                revision: self.0.session.revision(),
                response,
                error,
            })
            .map_err(|error| Error::Invalid(error.to_string()))?,
        );
        *self.0.last_reply.borrow_mut() = Some((command.id, Rc::clone(&reply)));
        Ok(Some(reply))
    }
}

/// Maps a private IDE build path to the user's editable source path.
///
/// Studio supplies `CRANPOSE_DEV_SOURCE_MAP` as an array of `[private, original]`
/// paths. Outside Studio the path is returned unchanged.
pub fn original_source_path(path: PathBuf) -> Result<PathBuf, Error> {
    let Some(maps) = std::env::var_os("CRANPOSE_DEV_SOURCE_MAP") else {
        return Ok(path);
    };
    let maps: Vec<(PathBuf, PathBuf)> = serde_json::from_str(&maps.to_string_lossy())
        .map_err(|error| Error::Invalid(format!("invalid development source map: {error}")))?;
    if let Some((_, original, suffix)) = maps
        .iter()
        .filter_map(|(private, original)| {
            path.strip_prefix(private)
                .ok()
                .map(|suffix| (private.components().count(), original, suffix))
        })
        .max_by_key(|(depth, _, _)| *depth)
    {
        return Ok(original.join(suffix));
    }
    Ok(path)
}

/// Renders a live document and serves editor/agent requests from its embedding host.
///
/// Native composable annotations are unchanged. The host transport is inactive
/// outside an embedded application; source watchers may use the same session.
#[crate::composable]
pub fn LiveHost(hosted: HostedSession) {
    let revision = hosted.0.session.observe_revision();
    let receiver = hosted.clone();
    CollectEvents(
        rememberHostMessages(REQUEST_CHANNEL),
        hosted.clone(),
        move |payload: String| match receiver.handle_json(&payload) {
            Ok(Some(reply)) => {
                send_to_host(RESPONSE_CHANNEL, &reply);
            }
            Ok(None) => {}
            Err(error) => eprintln!("live host: {error}"),
        },
    );
    let announce = hosted.clone();
    LaunchedEffect((hosted.clone(), revision), move |_| {
        if let Ok(payload) = announce.announcement()
            && send_to_host(READY_CHANNEL, &payload)
            && !announce.0.announced.replace(true)
        {
            println!(
                "{}",
                serde_json::json!({"cranposeLive": announce.0.document})
            );
        }
    });
    LiveView(hosted.0.session.clone());
}
