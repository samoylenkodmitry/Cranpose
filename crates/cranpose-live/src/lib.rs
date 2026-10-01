#![deny(missing_docs)]
//! Live UI programs sharing native Cranpose components and view models.

extern crate self as cranpose_live;

mod api;
mod error;
pub mod host;
mod limits;
mod program;
mod protocol;
mod registry;
mod schema;
mod session;
mod source;
pub mod widgets;

#[doc(hidden)]
pub use api::argument as __argument;
pub use api::{Api, ApiBuilder, MemberSchema};
pub use cranpose_macros::{composable, live_api};
pub use error::Error;
#[doc(hidden)]
pub use inventory::submit as __submit;
pub use limits::Limits;
pub use program::{Edit, Expression, Node, Patch, Program};
pub use protocol::{Request, Response};
pub use registry::{Catalogue, Component, ComponentSchema, Parameter, Registry};
pub use schema::{LiveValue, ValueType};
pub use session::{Action, Content, Invocation, LiveView, Session};
pub use source::parse_source;
