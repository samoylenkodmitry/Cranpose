use serde::{Deserialize, Serialize};

use crate::{Catalogue, Error, Patch, Program, Session};

/// Commands shared by editor integrations and agent transports.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Reads the linked component and bound view-model schemas.
    Catalogue,
    /// Reads the current editable document and revision.
    Snapshot,
    /// Reads the current value of an explicitly exported model state flow.
    State {
        /// Bound model namespace.
        api: String,
        /// Exported state getter name.
        member: String,
    },
    /// Translates a supported Rust entry function and replaces the document.
    Source {
        /// Revision on which the edit was based.
        base_revision: u64,
        /// Complete Rust file text.
        source: String,
        /// Entry function name.
        function: String,
    },
    /// Applies a transaction of structured UI edits.
    Patch {
        /// The edit transaction and its expected revision.
        patch: Patch,
    },
    /// Invokes an event on a node in the current document.
    Dispatch {
        /// Stable node identity.
        node: String,
        /// Event parameter name.
        event: String,
    },
}

/// A successful response; transport errors are returned separately as [`Error`].
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response<'a> {
    /// Current exported state, read without subscribing or invoking an action.
    State {
        /// Current document revision, independent of model state changes.
        revision: u64,
        /// Current flow value.
        value: serde_json::Value,
    },
    /// The APIs actually linked into the running application.
    Catalogue {
        /// Current document revision.
        revision: u64,
        /// Component and API contracts.
        catalogue: Catalogue<'a>,
    },
    /// The current document.
    Snapshot {
        /// Current document revision.
        revision: u64,
        /// Persistent document snapshot.
        program: Program,
    },
    /// A committed edit or completed event invocation.
    Accepted {
        /// Current document revision.
        revision: u64,
    },
}

impl Session {
    /// Handles a command on the UI thread, independent of its transport.
    pub fn handle(&self, request: Request) -> Result<Response<'_>, Error> {
        match request {
            Request::State { api, member } => {
                let export = self.registry().member(&api, &member)?;
                let crate::api::Member::State(read) = &export.member else {
                    return Err(Error::Invalid(format!("'{api}.{member}' is not state")));
                };
                Ok(Response::State {
                    revision: self.revision(),
                    value: read(false)?,
                })
            }
            Request::Catalogue => Ok(Response::Catalogue {
                revision: self.revision(),
                catalogue: self.registry().catalogue(),
            }),
            Request::Snapshot => Ok(Response::Snapshot {
                revision: self.revision(),
                program: self.program(),
            }),
            Request::Source {
                base_revision,
                source,
                function,
            } => {
                self.replace_source(base_revision, &source, &function)?;
                Ok(Response::Accepted {
                    revision: self.revision(),
                })
            }
            Request::Patch { patch } => {
                self.apply(patch)?;
                Ok(Response::Accepted {
                    revision: self.revision(),
                })
            }
            Request::Dispatch { node, event } => {
                self.dispatch(&node, &event)?;
                Ok(Response::Accepted {
                    revision: self.revision(),
                })
            }
        }
    }
}
