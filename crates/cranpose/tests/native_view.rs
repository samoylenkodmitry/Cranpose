use std::{cell::Cell, rc::Rc};

use cranpose::{
    native_view::{NativeView, NativeViewHost},
    prelude::*,
};
use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_render_pixels::PixelsRenderer;

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
