#![cfg(feature = "mcp")]

use std::{num::NonZeroUsize, rc::Rc, time::Duration};

use coroflow::{MutableStateFlow, StateFlow};
use cranpose_live::{Registry, Session, live_api, mcp::McpServer};
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

struct Counter(MutableStateFlow<i64>);

#[live_api]
impl Counter {
    #[live(state)]
    fn count(&self) -> StateFlow<i64> {
        self.0.as_state_flow()
    }

    #[live(action)]
    fn add(&self, amount: i64) {
        self.0.set(self.0.value() + amount);
    }
}

const SOURCE: &str = r#"fn Screen() { Column(|| {
    key("count", || Text(counter.count().to_string()));
    key("increment", || Button("Add".into(), || counter.add(1)));
}); }"#;

fn session() -> Session {
    let mut registry = Registry::discover().expect("discover");
    registry
        .bind("counter", Rc::new(Counter(MutableStateFlow::new(0))))
        .expect("model");
    let program = cranpose_live::parse_source(&registry, SOURCE, "Screen").expect("source");
    Session::new(registry, program).expect("session")
}

struct Client {
    writer: WriteHalf<DuplexStream>,
    reader: Lines<BufReader<ReadHalf<DuplexStream>>>,
    id: u64,
}

#[tokio::test(flavor = "current_thread")]
async fn disconnect_discards_a_queued_action_before_it_reaches_the_ui() {
    let session = session();
    let (server, mut requests) = McpServer::new(NonZeroUsize::new(1).expect("capacity"));
    let (app_io, agent_io) = tokio::io::duplex(65_536);
    let serving = tokio::spawn(async move {
        let (reader, writer) = tokio::io::split(app_io);
        server.serve_io(reader, writer).await.expect("serve MCP");
    });
    let mut client = Client::new(agent_io);
    client.initialize().await;
    client.send(json!({"jsonrpc":"2.0","id":42,"method":"tools/call","params":{
        "name":"cranpose_ui","arguments":{"request":{"method":"dispatch","node":"increment","event":"on_click"}}
    }})).await;
    let queued = requests.recv().await.expect("queued action");
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42}}))
        .await;
    client.request("ping", json!({})).await;
    queued.respond(&session);
    client.send(json!({"jsonrpc":"2.0","id":43,"method":"tools/call","params":{
        "name":"cranpose_ui","arguments":{"request":{"method":"dispatch","node":"increment","event":"on_click"}}
    }})).await;
    let queued = requests.recv().await.expect("second queued action");
    drop(client);
    tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("disconnect deadline")
        .expect("server task");
    queued.respond(&session);
    let value = session
        .handle(cranpose_live::Request::State {
            api: "counter".into(),
            member: "count".into(),
        })
        .expect("state");
    assert_eq!(serde_json::to_value(value).expect("json")["value"], 0);
}

impl Client {
    fn new(io: DuplexStream) -> Self {
        let (reader, writer) = tokio::io::split(io);
        Self {
            writer,
            reader: BufReader::new(reader).lines(),
            id: 0,
        }
    }

    async fn send(&mut self, value: Value) {
        self.writer
            .write_all(value.to_string().as_bytes())
            .await
            .expect("write");
        self.writer.write_all(b"\n").await.expect("newline");
    }

    async fn receive(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.reader.next_line())
            .await
            .expect("response deadline")
            .expect("read")
            .expect("response line");
        serde_json::from_str(&line).expect("JSON-RPC only")
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0", "id":self.id, "method":method, "params":params}))
            .await;
        let reply = self.receive().await;
        assert_eq!(reply["id"], self.id);
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    async fn initialize(&mut self) {
        let reply = self
            .request(
                "initialize",
                json!({
                    "protocolVersion":"2025-11-25", "capabilities":{},
                    "clientInfo":{"name":"live-ui-test","version":"1"}
                }),
            )
            .await;
        assert_eq!(reply["serverInfo"]["name"], "cranpose-live");
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
    }

    async fn call(&mut self, request: Value) -> Value {
        self.request(
            "tools/call",
            json!({"name":"cranpose_ui", "arguments":{"request":request}}),
        )
        .await
    }
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_client_discovers_edits_dispatches_and_observes_the_same_model() {
    let session = session();
    let (server, mut requests) = McpServer::new(NonZeroUsize::new(4).expect("capacity"));
    let (app_io, agent_io) = tokio::io::duplex(65_536);
    let serving = tokio::spawn(async move {
        let (reader, writer) = tokio::io::split(app_io);
        server.serve_io(reader, writer).await.expect("serve MCP");
    });
    let application = async {
        while let Some(call) = requests.recv().await {
            call.respond(&session);
        }
    };
    let agent = async {
        let mut client = Client::new(agent_io);
        client.initialize().await;
        let tools = client.request("tools/list", json!({})).await;
        assert_eq!(tools["tools"][0]["name"], "cranpose_ui");
        assert!(tools["tools"][0]["inputSchema"]["$defs"]["Request"].is_object());
        let catalogue = client.call(json!({"method":"catalogue"})).await;
        assert!(catalogue["structuredContent"]["catalogue"]["apis"]["counter"].is_array());
        let snapshot = client.call(json!({"method":"snapshot"})).await;
        assert_eq!(snapshot["structuredContent"]["revision"], 0);
        assert_eq!(
            client
                .call(json!({"method":"dispatch","node":"increment","event":"on_click"}))
                .await["isError"],
            false
        );
        let source = SOURCE
            .replace("Column", "Row")
            .replace("counter.add(1)", "counter.add(5)");
        let edited = client
            .call(json!({"method":"source","base_revision":0,"source":source,"function":"Screen"}))
            .await;
        assert_eq!(edited["structuredContent"]["revision"], 1);
        assert_eq!(
            client
                .call(json!({"method":"state","api":"counter","member":"count"}))
                .await["structuredContent"]["value"],
            1
        );
        client
            .call(json!({"method":"dispatch","node":"increment","event":"on_click"}))
            .await;
        assert_eq!(
            client
                .call(json!({"method":"state","api":"counter","member":"count"}))
                .await["structuredContent"]["value"],
            6
        );
        let patch = json!({"method":"patch","patch":{"base_revision":1,"edits":[{
            "kind":"set_argument","target":"increment","name":"label",
            "value":{"kind":"literal","value":"Agent edited"}
        }]}});
        assert_eq!(
            client.call(patch.clone()).await["structuredContent"]["revision"],
            2
        );
        assert_eq!(client.call(patch).await["isError"], true);
        assert_eq!(client.call(json!({"method":"source","base_revision":2,"source":"broken","function":"Screen"})).await["isError"], true);
        assert_eq!(
            client
                .call(json!({"method":"state","api":"counter","member":"add"}))
                .await["isError"],
            true
        );
        let snapshot = client.call(json!({"method":"snapshot"})).await;
        assert_eq!(snapshot["structuredContent"]["revision"], 2);
        assert_eq!(
            snapshot["structuredContent"]["program"]["root"]["component"],
            "cranpose_live::widgets::Row"
        );
        assert_eq!(
            snapshot["structuredContent"]["program"]["root"]["children"][1]["arguments"]["label"]["value"],
            "Agent edited"
        );
        drop(client);
        serving.await.expect("server task");
    };
    tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(application, agent);
    })
    .await
    .expect("MCP disconnect closes request queue");
}

#[tokio::test(flavor = "current_thread")]
async fn full_or_closed_queues_reply_without_executing_commands() {
    let session = session();
    let (server, mut requests) = McpServer::new(NonZeroUsize::new(1).expect("capacity"));
    let (app_io, agent_io) = tokio::io::duplex(65_536);
    let serving = tokio::spawn(async move {
        let (reader, writer) = tokio::io::split(app_io);
        server.serve_io(reader, writer).await.expect("serve MCP");
    });
    let mut client = Client::new(agent_io);
    client.initialize().await;
    let command = json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{
        "name":"cranpose_ui","arguments":{"request":{"method":"dispatch","node":"increment","event":"on_click"}}
    }});
    client.send(command.clone()).await;
    let first = requests.recv().await.expect("first command");
    let mut second = command.clone();
    second["id"] = json!(11);
    client.send(second).await;
    client.request("ping", json!({})).await;
    let mut third = command;
    third["id"] = json!(12);
    client.send(third).await;
    let full = client.receive().await;
    assert_eq!(full["id"], 12);
    assert_eq!(full["result"]["isError"], true);
    first.respond(&session);
    assert_eq!(client.receive().await["id"], 10);
    drop(requests);
    let closed = client.receive().await;
    assert_eq!(closed["id"], 11);
    assert_eq!(closed["result"]["isError"], true);
    assert_eq!(
        client.call(json!({"method":"snapshot"})).await["isError"],
        true
    );
    let value = session
        .handle(cranpose_live::Request::State {
            api: "counter".into(),
            member: "count".into(),
        })
        .expect("state");
    assert_eq!(serde_json::to_value(value).expect("json")["value"], 1);
    drop(client);
    serving.await.expect("server stopped");
}
