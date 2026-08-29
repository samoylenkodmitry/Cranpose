use cranpose_core::MutableState;
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn simple_button_app(clicked_count: MutableState<i32>) {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Clicks: {}", clicked_count.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                {
                    let count = clicked_count;
                    move || {
                        count.set(count.get() + 1);
                    }
                },
                || {
                    Text(
                        "Click me",
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[test]
fn test_button_creates_valid_composition() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let clicked_count = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        let count = clicked_count;
        move || {
            simple_button_app(count);
        }
    })
    .expect("initial render succeeds");

    assert_eq!(
        clicked_count.get(),
        0,
        "Button should not have been clicked yet"
    );

    let node_count = rule.applier_mut().len();
    assert!(
        node_count >= 4,
        "Should have at least 4 nodes (Column, Text, Button, ButtonSpec, Button's Text)"
    );
}

#[composable]
fn multi_button_app(button1_clicks: MutableState<i32>, button2_clicks: MutableState<i32>) {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Button 1 clicks: {}", button1_clicks.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                {
                    let clicks = button1_clicks;
                    move || {
                        clicks.set(clicks.get() + 1);
                    }
                },
                || {
                    Text(
                        "Button 1",
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                },
            );

            Text(
                format!("Button 2 clicks: {}", button2_clicks.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                {
                    let clicks = button2_clicks;
                    move || {
                        clicks.set(clicks.get() + 10);
                    }
                },
                || {
                    Text(
                        "Button 2",
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[test]
fn test_multiple_buttons_in_composition() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let button1_clicks = MutableState::with_runtime(0, runtime.clone());
    let button2_clicks = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        let clicks1 = button1_clicks;
        let clicks2 = button2_clicks;
        move || {
            multi_button_app(clicks1, clicks2);
        }
    })
    .expect("initial render succeeds");

    assert_eq!(button1_clicks.get(), 0);
    assert_eq!(button2_clicks.get(), 0);

    let node_count = rule.applier_mut().len();
    assert!(
        node_count >= 7,
        "Should have at least 7 nodes for the two button app"
    );
}
