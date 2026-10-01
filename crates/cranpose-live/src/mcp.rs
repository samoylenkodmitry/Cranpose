//! MCP access to the same UI-thread session used by source watching and Studio.

use std::{
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll},
};

use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{mpsc, oneshot, watch},
};

use crate::{Error, Request, Session};

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    request: Request,
}

/// An MCP command waiting to be executed on the application's UI thread.
pub struct McpCall {
    arguments: Value,
    reply: oneshot::Sender<Result<Value, String>>,
    disconnected: watch::Receiver<bool>,
    cancelled: tokio_util::sync::CancellationToken,
}

impl McpCall {
    /// Executes against the existing session and replies to the agent.
    ///
    /// Cancelled calls are discarded before execution. Once an action has run,
    /// cancelling its reply cannot undo that action; callers must not retry it
    /// blindly. Source and patch requests retain the session's revision checks.
    pub fn respond(self, session: &Session) {
        if self.reply.is_closed() || *self.disconnected.borrow() || self.cancelled.is_cancelled() {
            return;
        }
        let result = serde_json::from_value::<Arguments>(self.arguments)
            .map_err(Error::from)
            .and_then(|arguments| session.handle(arguments.request))
            .and_then(|response| serde_json::to_value(response).map_err(Error::from))
            .map_err(|error| error.to_string());
        let _ = self.reply.send(result);
    }
}

/// An MCP server forwarding commands through a bounded queue to a live session.
///
/// Poll the returned receiver on the UI thread and call [`McpCall::respond`].
/// Dropping the receiver disconnects waiting requests. The server never moves
/// native models or composition state to its I/O executor.
pub struct McpServer {
    sender: mpsc::Sender<McpCall>,
    tool: Tool,
    disconnected: watch::Sender<bool>,
}

impl McpServer {
    /// Creates the server and its UI-thread request receiver.
    pub fn new(capacity: NonZeroUsize) -> (Self, mpsc::Receiver<McpCall>) {
        let (sender, receiver) = mpsc::channel(capacity.get());
        let Value::Object(schema) = schemars::schema_for!(Arguments).to_value() else {
            unreachable!("MCP arguments are an object");
        };
        let tool = Tool::new(
            "cranpose_ui",
            "Inspect and edit this running Cranpose UI. Start with catalogue and snapshot. \
             Only registered components and explicitly exposed model APIs are available. \
             Source/patch require the latest base_revision. State reads a live model value; \
             dispatch invokes a node event and may change application state. Edits affect \
             this running session, not source files. Do not retry an uncertain dispatch.",
            schema,
        );
        let (disconnected, _) = watch::channel(false);
        (
            Self {
                sender,
                tool,
                disconnected,
            },
            receiver,
        )
    }

    /// Serves MCP over standard input/output until the client disconnects.
    ///
    /// Run on a Tokio I/O executor. All other application output must use stderr;
    /// stdout belongs exclusively to MCP for the duration of this call.
    pub async fn serve_stdio(self) -> Result<(), Error> {
        let (reader, writer) = rmcp::transport::stdio();
        self.serve_io(reader, writer).await
    }

    /// Serves MCP on an asynchronous byte stream, cancelling queued work on EOF.
    ///
    /// Like [`Self::serve_stdio`], the writer must be reserved for MCP messages.
    pub async fn serve_io<R, W>(self, reader: R, writer: W) -> Result<(), Error>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let reader = ConnectionReader {
            inner: reader,
            disconnected: self.disconnected.clone(),
        };
        let service = self
            .serve((reader, writer))
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?;
        service
            .waiting()
            .await
            .map_err(|error| Error::Invalid(error.to_string()))
            .and_then(|reason| match reason {
                rmcp::service::QuitReason::JoinError(error) => {
                    Err(Error::Invalid(error.to_string()))
                }
                _ => Ok(()),
            })
    }
}

fn failed(message: impl Into<String>) -> CallToolResponse {
    CallToolResult::error(vec![ContentBlock::text(message.into())]).into()
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = Implementation::new("cranpose-live", env!("CARGO_PKG_VERSION"));
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![self.tool.clone()]))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        (name == self.tool.name).then(|| self.tool.clone())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != self.tool.name {
            return Err(ErrorData::invalid_params("unknown Cranpose tool", None));
        }
        let (reply, response) = oneshot::channel();
        let call = McpCall {
            arguments: Value::Object(request.arguments.unwrap_or_default()),
            reply,
            disconnected: self.disconnected.subscribe(),
            cancelled: context.ct.clone(),
        };
        if let Err(error) = self.sender.try_send(call) {
            return Ok(failed(match error {
                mpsc::error::TrySendError::Full(_) => {
                    "UI request queue is full; no command was executed"
                }
                mpsc::error::TrySendError::Closed(_) => "Live UI session is closed",
            }));
        }
        let mut disconnected = self.disconnected.subscribe();
        tokio::select! {
            biased;
            () = context.ct.cancelled() => Ok(failed("Request cancelled")),
            _ = disconnected.wait_for(|closed| *closed) => Ok(failed("Client disconnected")),
            result = response => match result {
                Ok(Ok(value)) => {
                    let mut result = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
                    result.structured_content = Some(value);
                    Ok(result.into())
                }
                Ok(Err(error)) => Ok(failed(error)),
                Err(_) => Ok(failed("Live UI session closed before replying")),
            },
        }
    }
}

struct ConnectionReader<R> {
    inner: R,
    disconnected: watch::Sender<bool>,
}

impl<R: AsyncRead + Unpin> AsyncRead for ConnectionReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buffer.filled().len();
        let has_capacity = buffer.remaining() > 0;
        let result = Pin::new(&mut this.inner).poll_read(cx, buffer);
        if matches!(&result, Poll::Ready(Err(_)))
            || (has_capacity
                && matches!(&result, Poll::Ready(Ok(())))
                && buffer.filled().len() == before)
        {
            this.disconnected.send_replace(true);
        }
        result
    }
}

impl<R> Drop for ConnectionReader<R> {
    fn drop(&mut self) {
        self.disconnected.send_replace(true);
    }
}
