use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use coroflow::{MutableStateFlow, StateFlow};
use cranpose_core::location_key;
use cranpose_live::{
    Edit, Error, Expression, LiveView, Patch, Registry, Session, composable, live_api,
};
use cranpose_ui::{LayoutBox, LayoutEngine, Size, TestComposition, run_test_composition};
use serde_json::json;

struct Counter {
    value: MutableStateFlow<i64>,
}

#[live_api]
impl Counter {
    #[live(state)]
    fn count(&self) -> StateFlow<i64> {
        self.value.as_state_flow()
    }

    #[live(action)]
    fn add(&self, amount: i64) {
        self.value.set(self.value.value() + amount);
    }

    #[live(action)]
    fn reset(&self) {
        self.value.set(0);
    }

    #[live(action)]
    #[cfg(any())]
    fn disabled_action(&self) {}
}

#[composable]
fn Caption(text: String) {
    cranpose_live::widgets::Text(text);
}

#[composable]
fn NativeOnly(value: usize) {
    cranpose_live::widgets::Text(value.to_string());
}

#[composable]
fn Sticky() {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let identity =
        cranpose_core::remember(|| NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    cranpose_live::widgets::Text(identity.with(|id| format!("remembered:{id}")));
}

#[composable]
#[cfg(any())]
fn Disabled() {}

fn setup() -> (Rc<Counter>, Registry) {
    let model = Rc::new(Counter {
        value: MutableStateFlow::new(0),
    });
    let mut registry = Registry::discover().expect("discover components");
    registry
        .bind("counter", Rc::clone(&model))
        .expect("bind model");
    (model, registry)
}

const INITIAL: &str = r#"
fn Screen() {
    Column(|| {
        key("value", || Caption(counter.count().collectAsState().get().to_string()));
        key("increment", || Button("Add".into(), || counter.add(1)));
    });
}
"#;

fn session(registry: Registry, source: &str) -> Session {
    let program =
        cranpose_live::parse_source(&registry, source, "Screen").expect("translate source");
    Session::new(registry, program).expect("validate program")
}

fn render(composition: &mut TestComposition, session: &Session) {
    composition
        .render(location_key("live-test", 1, 1), || {
            LiveView(session.clone());
        })
        .expect("render");
}

fn texts(node: &LayoutBox, output: &mut Vec<String>) {
    if let Some(text) = node.node_data.modifier_slices().text_content() {
        output.push(text.to_owned());
    }
    for child in &node.children {
        texts(child, output);
    }
}

fn visible_text(composition: &mut TestComposition) -> Vec<String> {
    let mut actual = Vec::new();
    let root = composition.root().expect("UI root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let tree = applier
        .compute_layout(root, Size::new(800.0, 600.0))
        .expect("layout");
    texts(tree.root(), &mut actual);
    applier.clear_runtime_handle();
    actual
}

fn expect_text(composition: &mut TestComposition, session: &Session, expected: &[&str]) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        composition.with_app_context(|| composition.runtime_handle().drain_ui());
        if composition.should_render() {
            render(composition, session);
        }
        composition.process_invalid_scopes().expect("recompose");
        composition
            .flush_pending_node_updates()
            .expect("apply updates");
        let actual = visible_text(composition);
        if actual == expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "expected {expected:?}, rendered {actual:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(session.take_errors().is_empty());
}

#[test]
fn plain_composable_exports_signatures_and_native_only_diagnostics() {
    let (_, registry) = setup();
    let catalogue = registry.catalogue();
    let caption = catalogue
        .components
        .iter()
        .find(|c| c.name.ends_with("::Caption"))
        .expect("automatic registration");
    assert!(caption.unavailable.is_none());
    assert_eq!(caption.parameters[0].name, "text");
    assert!(caption.signature.contains("String"));
    let native = catalogue
        .components
        .iter()
        .find(|c| c.name.ends_with("::NativeOnly"))
        .expect("native metadata");
    assert!(native.signature.contains("usize"));
    assert!(native.unavailable.is_some());
    let json = serde_json::to_value(catalogue).expect("serializable catalogue");
    assert!(
        json["apis"]["counter"]
            .as_array()
            .expect("api members")
            .iter()
            .any(|m| m["name"] == "count")
    );
    run_test_composition(|| NativeOnly(7));
}

#[test]
fn embedded_commands_route_correlate_and_preserve_model_state() {
    use cranpose_live::host::HostedSession;

    let (model, registry) = setup();
    let session = session(registry, INITIAL);
    let hosted = HostedSession::new(session, "/project/screen.rs".into(), "Screen".into());
    let ready: serde_json::Value =
        serde_json::from_str(&hosted.announcement().expect("announce")).expect("discovery JSON");
    let document = ready["document"].as_str().expect("address");
    let request = |id, request| json!({"id":id,"document":document,"request":request}).to_string();
    let call = |id, command| -> serde_json::Value {
        let reply = hosted
            .handle_json(&request(id, command))
            .expect("handle")
            .expect("addressed reply");
        serde_json::from_str(&reply).expect("reply JSON")
    };
    assert_eq!(ready["file"], "/project/screen.rs");
    assert_eq!(
        call(1, json!({"method":"catalogue"}))["response"]["kind"],
        "catalogue"
    );
    let dispatch = request(
        2,
        json!({"method":"dispatch","node":"increment","event":"on_click"}),
    );
    let first = hosted.handle_json(&dispatch).expect("dispatch");
    assert_eq!(model.value.value(), 1);
    assert_eq!(hosted.handle_json(&dispatch).expect("host replay"), first);
    assert_eq!(
        model.value.value(),
        1,
        "a replay must not invoke the action twice"
    );
    assert!(
        hosted
            .handle_json(&request(
                1,
                json!({"method":"dispatch","node":"increment","event":"on_click"})
            ))
            .is_err()
    );
    let changed = INITIAL
        .replace("Column", "Row")
        .replace("Add", "Add five")
        .replace("counter.add(1)", "counter.add(5)");
    let applied = call(
        3,
        json!({"method":"source","base_revision":0,"source":changed,"function":"Screen"}),
    );
    assert_eq!(applied["id"], 3);
    assert_eq!(applied["revision"], 1);
    assert_eq!(model.value.value(), 1);
    let stale = call(
        4,
        json!({"method":"source","base_revision":0,"source":INITIAL,"function":"Screen"}),
    );
    assert!(stale["error"].as_str().is_some());
    assert_eq!(stale["revision"], 1);
    let snapshot = call(5, json!({"method":"snapshot"}));
    assert_eq!(
        snapshot["response"]["program"]["root"]["component"],
        "cranpose_live::widgets::Row"
    );
    assert!(hosted.handle_json(&json!({"id":6,"document":"another document","request":{"method":"dispatch","node":"increment","event":"on_click"}}).to_string()).expect("other document").is_none());
    assert_eq!(model.value.value(), 1);
    call(
        6,
        json!({"method":"dispatch","node":"increment","event":"on_click"}),
    );
    assert_eq!(model.value.value(), 6);
    assert!(
        call(7, json!({"method":"unknown"}))["error"]
            .as_str()
            .is_some()
    );
}

#[test]
fn embedded_host_messages_edit_a_rendered_document_without_restarting_it() {
    use std::sync::{Arc, Mutex};

    use cranpose_live::host::{
        HostedSession, LiveHost, READY_CHANNEL, REQUEST_CHANNEL, RESPONSE_CHANNEL,
    };
    use cranpose_services::{
        HostMessage, clear_host_messages, clear_host_outbox, install_host_outbox,
        publish_host_message,
    };

    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            clear_host_outbox();
            clear_host_messages();
        }
    }
    let _cleanup = Cleanup;
    clear_host_messages();
    let received = Arc::new(Mutex::new(Vec::<HostMessage>::new()));
    let outbox = Arc::clone(&received);
    install_host_outbox(move |message| outbox.lock().expect("outbox").push(message));
    let (model, registry) = setup();
    let session = session(registry, INITIAL);
    let hosted = HostedSession::new(
        session.clone(),
        "/project/screen.rs".into(),
        "Screen".into(),
    );
    let ready: serde_json::Value =
        serde_json::from_str(&hosted.announcement().expect("announce")).expect("JSON");
    let mut composition = run_test_composition(|| {});
    {
        let mut wait_for = |channel: &str, id: Option<u64>| {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                composition
                    .render(location_key("host-transport", 1, 1), || {
                        LiveHost(hosted.clone());
                    })
                    .expect("render hosted UI");
                composition.with_app_context(|| composition.runtime_handle().drain_ui());
                composition.process_invalid_scopes().expect("recompose");
                composition.flush_pending_node_updates().expect("apply");
                let found = received.lock().expect("received").iter().any(|message| {
                    message.channel == channel
                        && id.is_none_or(|id| {
                            serde_json::from_str::<serde_json::Value>(&message.payload)
                                .is_ok_and(|value| value["id"] == id)
                        })
                });
                if found {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "missing host response on {channel}"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        wait_for(READY_CHANNEL, None);
        let command = |id, request| {
            HostMessage::new(
                REQUEST_CHANNEL,
                json!({"id":id,"document":ready["document"],"request":request}).to_string(),
            )
        };
        publish_host_message(command(
            1,
            json!({"method":"dispatch","node":"increment","event":"on_click"}),
        ));
        wait_for(RESPONSE_CHANNEL, Some(1));
        assert_eq!(model.value.value(), 1);
        publish_host_message(command(
            2,
            json!({"method":"source","base_revision":0,
            "source":INITIAL.replace("Column", "Row").replace("Add", "Live in Studio"),"function":"Screen"}),
        ));
        wait_for(RESPONSE_CHANNEL, Some(2));
        assert_eq!(session.revision(), 1);
        assert_eq!(model.value.value(), 1);
        assert_eq!(visible_text(&mut composition), ["1", "Live in Studio"]);
    }
}

#[test]
fn source_and_agent_edits_update_real_widgets_and_keep_viewmodel_state() {
    let (model, registry) = setup();
    let session = session(registry, INITIAL);
    let mut composition = run_test_composition(|| {});
    render(&mut composition, &session);
    expect_text(&mut composition, &session, &["0", "Add"]);
    session
        .dispatch("increment", "on_click")
        .expect("native action");
    expect_text(&mut composition, &session, &["1", "Add"]);

    let edited = INITIAL
        .replace("Column", "Row")
        .replace("counter.add(1)", "counter.add(5)");
    session
        .replace_source(0, &edited, "Screen")
        .expect("structural source edit");
    expect_text(&mut composition, &session, &["1", "Add"]);
    session
        .dispatch("increment", "on_click")
        .expect("new event binding");
    expect_text(&mut composition, &session, &["6", "Add"]);

    let patch = json!({"base_revision": 1, "edits": [{
        "kind": "set_argument", "target": "increment", "name": "label",
        "value": {"kind": "literal", "value": "Agent changed this"}
    }]});
    session
        .apply(serde_json::from_value(patch).expect("agent patch"))
        .expect("apply agent edit");
    expect_text(&mut composition, &session, &["6", "Agent changed this"]);
    model.add(4);
    expect_text(&mut composition, &session, &["10", "Agent changed this"]);
}

#[test]
fn invalid_and_stale_edits_leave_the_last_good_document_and_actions() {
    let (model, registry) = setup();
    let session = session(registry, INITIAL);
    let invalid = Patch {
        base_revision: 0,
        edits: vec![
            Edit::SetArgument {
                target: "increment".into(),
                name: "label".into(),
                value: Expression::Literal {
                    value: json!("partial"),
                },
            },
            Edit::SetArgument {
                target: "missing".into(),
                name: "label".into(),
                value: Expression::Literal { value: json!(0) },
            },
        ],
    };
    assert!(session.apply(invalid).is_err());
    assert_eq!(session.revision(), 0);
    assert!(
        session
            .replace_source(0, "fn Screen() { Missing(); }", "Screen")
            .is_err()
    );
    session
        .replace_source(
            0,
            &INITIAL.replace("counter.add(1)", "counter.add(2)"),
            "Screen",
        )
        .expect("valid edit");
    assert!(matches!(
        session.replace_source(0, INITIAL, "Screen"),
        Err(Error::Stale {
            expected: 1,
            received: 0
        })
    ));
    session
        .dispatch("increment", "on_click")
        .expect("old document still works");
    assert_eq!(model.value.value(), 2);
    let mut composition = run_test_composition(|| {});
    render(&mut composition, &session);
    expect_text(&mut composition, &session, &["2", "Add"]);
}

#[test]
fn event_arguments_read_current_state_at_dispatch_time() {
    let (model, registry) = setup();
    let source = INITIAL.replace(
        "counter.add(1)",
        "counter.add(counter.count().collectAsState().get())",
    );
    let session = session(registry, &source);
    model.add(3);
    session
        .dispatch("increment", "on_click")
        .expect("dynamic event");
    assert_eq!(model.value.value(), 6);
    session
        .dispatch("increment", "on_click")
        .expect("fresh dynamic event");
    assert_eq!(model.value.value(), 12);
}

#[test]
fn duplicate_saves_and_empty_patches_do_not_advance_the_document() {
    let (_, registry) = setup();
    let session = session(registry, INITIAL);
    assert_eq!(
        session
            .replace_source(0, &format!("{INITIAL}\n"), "Screen")
            .expect("same program"),
        0
    );
    assert_eq!(
        session
            .apply(Patch {
                base_revision: 0,
                edits: Vec::new()
            })
            .expect("empty patch"),
        0
    );
}

#[test]
fn removing_a_flow_reader_releases_its_subscription() {
    let (model, registry) = setup();
    let session = session(registry, INITIAL);
    let mut composition = run_test_composition(|| {});
    render(&mut composition, &session);
    expect_text(&mut composition, &session, &["0", "Add"]);
    assert_eq!(model.count().subscription_count().value(), 1);
    session
        .replace_source(0, "fn Screen() { Text(\"removed\".into()); }", "Screen")
        .expect("remove reader");
    expect_text(&mut composition, &session, &["removed"]);
    assert_eq!(model.count().subscription_count().value(), 0);
}

#[test]
fn application_limits_reject_oversized_documents() {
    let (_, mut registry) = setup();
    registry.limits.source_bytes = 4;
    assert!(cranpose_live::parse_source(&registry, INITIAL, "Screen").is_err());
    registry.limits = cranpose_live::Limits {
        nodes: 1,
        ..Default::default()
    };
    let program = cranpose_live::parse_source(&registry, INITIAL, "Screen").expect("source");
    assert!(Session::new(registry, program).is_err());
}

#[test]
fn incompatible_values_and_duplicate_identities_are_rejected() {
    let (_, registry) = setup();
    let program = cranpose_live::parse_source(
        &registry,
        &INITIAL.replace("\"Add\".into()", "12"),
        "Screen",
    )
    .expect("syntactically valid");
    assert!(Session::new(registry, program).is_err());
    let (_, registry) = setup();
    let program = cranpose_live::parse_source(
        &registry,
        &INITIAL.replace("\"increment\"", "\"value\""),
        "Screen",
    )
    .expect("syntactically valid");
    assert!(Session::new(registry, program).is_err());
}

#[test]
fn keyed_native_remember_survives_sibling_insertion() {
    let (_, registry) = setup();
    let source = r#"fn Screen() { Column(|| { key("sticky", || Sticky()); }); }"#;
    let session = session(registry, source);
    let mut composition = run_test_composition(|| {});
    render(&mut composition, &session);
    let before = visible_text(&mut composition);
    assert_eq!(before.len(), 1);
    let edited = source.replace("key(", r#"Text("inserted".into()); key("#);
    session
        .replace_source(0, &edited, "Screen")
        .expect("insert sibling");
    expect_text(&mut composition, &session, &["inserted", &before[0]]);
}

#[test]
fn transport_catalogue_snapshot_and_patch_share_one_revision() {
    let (_, registry) = setup();
    let session = session(registry, INITIAL);
    let request = serde_json::from_str(r#"{"method":"catalogue"}"#).expect("catalogue request");
    let reply =
        serde_json::to_value(session.handle(request).expect("catalogue")).expect("json reply");
    assert_eq!(reply["revision"], 0);
    assert!(
        !reply["catalogue"]["components"]
            .as_array()
            .expect("components")
            .is_empty()
    );
    let request = serde_json::from_str(r#"{"method":"source","base_revision":0,"source":"fn Screen() { Text(\"hello\".into()); }","function":"Screen"}"#).expect("source request");
    session.handle(request).expect("source accepted");
    let snapshot = serde_json::to_value(
        session
            .handle(cranpose_live::Request::Snapshot)
            .expect("snapshot"),
    )
    .expect("json snapshot");
    assert_eq!(snapshot["revision"], 1);
    assert_eq!(
        snapshot["program"]["root"]["arguments"]["text"]["value"],
        "hello"
    );
}

#[test]
fn state_requests_read_current_flows_and_never_invoke_actions() {
    let (model, registry) = setup();
    let session = session(registry, INITIAL);
    let read = || {
        serde_json::to_value(
            session
                .handle(cranpose_live::Request::State {
                    api: "counter".into(),
                    member: "count".into(),
                })
                .expect("read state"),
        )
        .expect("serialize state")
    };
    assert_eq!(read()["value"], 0);
    model.add(7);
    assert_eq!(read(), json!({"kind":"state","revision":0,"value":7}));
    for (api, member) in [
        ("counter", "reset"),
        ("missing", "count"),
        ("counter", "missing"),
    ] {
        assert!(
            session
                .handle(cranpose_live::Request::State {
                    api: api.into(),
                    member: member.into()
                })
                .is_err()
        );
    }
    assert_eq!(read()["value"], 7);
}

#[test]
fn unsupported_rust_reports_a_diagnostic_without_committing() {
    let (_, registry) = setup();
    let session = session(registry, INITIAL);
    for source in [
        "async fn Screen() { Text(\"a\".into()); }",
        "fn Screen<T>() { Text(\"a\".into()); }",
        "fn Screen() { let label = \"a\"; Text(label); }",
        "fn Screen() { Column(async || Text(\"a\".into())); }",
        "fn Screen() { Text(\"a\".into::<String>()); }",
    ] {
        assert!(
            session.replace_source(0, source, "Screen").is_err(),
            "{source}"
        );
        assert_eq!(session.revision(), 0);
    }
}
