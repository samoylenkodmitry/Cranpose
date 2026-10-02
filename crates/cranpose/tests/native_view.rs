use std::{cell::Cell, rc::Rc};

use cranpose::{
    native_view::{NativeView, NativeViewHost},
    prelude::*,
};
use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_render_pixels::PixelsRenderer;

#[test]
fn shared_webview_preserves_identity_and_delivers_page_events() {
    use std::cell::RefCell;

    use cranpose::{WebView, WebViewEvent};

    let host = NativeViewHost::default();
    let content_host = host.clone();
    let url = Rc::new(Cell::new(None::<MutableState<&'static str>>));
    let content_url = Rc::clone(&url);
    let events = Rc::new(RefCell::new(Vec::new()));
    let content_events = Rc::clone(&events);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        default_root_key(),
        move || {
            let page = rememberMutableStateOf(|| "https://example.com");
            content_url.set(Some(page));
            let events = Rc::clone(&content_events);
            content_host.provide(|| {
                WebView(
                    page.value(),
                    Modifier::empty().size_points(200.0, 120.0),
                    move |event| {
                        events.borrow_mut().push(event);
                    },
                );
            });
        },
        (320, 240),
        (320.0, 240.0),
    );
    let initial = host.layout(shell.layout_tree());
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].value, "https://example.com");
    assert!(host.dispatch(initial[0].id, "loaded:https://example.com"));
    assert_eq!(
        *events.borrow(),
        [WebViewEvent::Loaded("https://example.com".into())]
    );
    url.get()
        .expect("composed URL state")
        .set("https://example.org");
    shell.update();
    let updated = host.layout(shell.layout_tree());
    assert_eq!(updated[0].id, initial[0].id);
    assert_eq!(updated[0].value, "https://example.org");
    assert!(host.dispatch(updated[0].id, "error:offline"));
    assert_eq!(
        events.borrow().last(),
        Some(&WebViewEvent::Failed("offline".into()))
    );
    drop(shell);
    assert!(!host.dispatch(updated[0].id, "loaded:stale"));
}

#[test]
fn native_slots_follow_layout_updates_events_and_disposal() {
    let host = NativeViewHost::default();
    let content_host = host.clone();
    let controls = Rc::new(Cell::new(None::<MutableState<bool>>));
    let content_controls = Rc::clone(&controls);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        default_root_key(),
        move || {
            let show = rememberMutableStateOf(|| true);
            let count = rememberMutableStateOf(|| 0);
            content_controls.set(Some(show));
            let host = content_host.clone();
            Column(
                Modifier::empty().padding(12.0),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                move || {
                    Spacer(Modifier::empty().height(20.0));
                    if show.value() {
                        NativeView(
                            host.clone(),
                            "web",
                            &format!("count={}", count.value()),
                            Modifier::empty().width(100.0).height(60.0),
                            move |event| {
                                if event == "increment" {
                                    count.set(count.value() + 1);
                                }
                            },
                        );
                    }
                },
            );
        },
        (320, 240),
        (320.0, 240.0),
    );
    let slots = host.layout(shell.layout_tree());
    assert_eq!(slots.len(), 1);
    assert_eq!(
        slots[0].bounds,
        Rect {
            x: 12.0,
            y: 40.0,
            width: 100.0,
            height: 60.0
        }
    );
    let id = slots[0].id;
    assert!(host.dispatch(id, "increment"));
    shell.update();
    let slots = host.layout(shell.layout_tree());
    assert_eq!(slots[0].id, id);
    assert_eq!(slots[0].value, "count=1");

    controls.get().expect("composed controls").set(false);
    shell.update();
    assert!(host.layout(shell.layout_tree()).is_empty());
    assert!(!host.dispatch(id, "increment"));

    controls.get().expect("composed controls").set(true);
    shell.update();
    let remounted = host.layout(shell.layout_tree());
    assert_eq!(remounted.len(), 1);
    assert_ne!(remounted[0].id, id);
    drop(shell);
    assert!(!host.dispatch(remounted[0].id, "increment"));
}

#[test]
fn hosts_do_not_share_native_events() {
    fn component(host: NativeViewHost) -> AppShell<PixelsRenderer> {
        AppShell::new_with_size(
            PixelsRenderer::new(),
            default_root_key(),
            move || {
                let count = rememberMutableStateOf(|| 0);
                NativeView(
                    host.clone(),
                    "web",
                    &count.value().to_string(),
                    Modifier::empty().width(100.0).height(60.0),
                    move |_| count.set(count.value() + 1),
                );
            },
            (320, 240),
            (320.0, 240.0),
        )
    }
    let first = NativeViewHost::default();
    let second = NativeViewHost::default();
    let mut a = component(first.clone());
    let mut b = component(second.clone());
    let id = first.layout(a.layout_tree())[0].id;
    assert!(first.dispatch(id, "increment"));
    a.update();
    b.update();
    assert_eq!(first.layout(a.layout_tree())[0].value, "1");
    assert_eq!(second.layout(b.layout_tree())[0].value, "0");
}

#[test]
fn changing_factory_retires_old_events() {
    let host = NativeViewHost::default();
    let content_host = host.clone();
    let kind = Rc::new(Cell::new(None::<MutableState<bool>>));
    let content_kind = Rc::clone(&kind);
    let received = Rc::new(Cell::new(0));
    let content_received = Rc::clone(&received);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        default_root_key(),
        move || {
            let alternate = rememberMutableStateOf(|| false);
            content_kind.set(Some(alternate));
            let received = Rc::clone(&content_received);
            NativeView(
                content_host.clone(),
                if alternate.value() { "map" } else { "web" },
                "",
                Modifier::empty().width(100.0).height(60.0),
                move |_| received.set(received.get() + 1),
            );
        },
        (320, 240),
        (320.0, 240.0),
    );
    let old = host.layout(shell.layout_tree())[0].id;
    kind.get().expect("mounted").set(true);
    shell.update();
    let replacement = host.layout(shell.layout_tree());
    assert_eq!(replacement[0].kind, "map");
    assert!(!host.dispatch(old, "late completion from disposed WebView"));
    assert!(host.dispatch(replacement[0].id, "current event"));
    assert_eq!(received.get(), 1);
}
