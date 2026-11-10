use cranpose_core::LaunchedEffect;
use cranpose_ui::{
    composable, Brush, Button, ButtonSpec, Color, CornerRadii, Modifier, Spacer, Text, TextStyle,
};

use super::demo_random_u32;

// A line with the last fetched value over the button that fetches the next.
#[composable]
pub(super) fn FetchAsyncValue() {
    let async_message = cranpose_core::rememberMutableStateOf(|| {
        "Tap \"Fetch async value\" to run background work".to_string()
    });
    let fetch_request = cranpose_core::rememberMutableStateOf(|| 0u64);
    let fetch_key = fetch_request.get();
    LaunchedEffect(fetch_key, async move |_scope| {
        if fetch_key == 0 {
            return;
        }
        _scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return String::new();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use std::{thread, time::Duration};
                    for _ in 0..5 {
                        if token.is_cancelled() {
                            return String::new();
                        }
                        thread::sleep(Duration::from_millis(80));
                    }
                }
                let val = demo_random_u32() % 1000;
                format!("Background fetch #{fetch_key}: {val}")
            },
            move |value| {
                if value.is_empty() {
                    return;
                }
                async_message.set(value);
            },
        );
    });
    Text(
        async_message.get(),
        Modifier::empty()
            .background(Color(0.1, 0.18, 0.32, 0.6))
            .rounded_corners(14.0)
            .padding(10.0),
        TextStyle::default(),
    );

    Spacer(Modifier::empty().size_points(0.0, 12.0));

    Button(
        Modifier::empty()
            .rounded_corners(16.0)
            .draw_with_cache(|cache| {
                cache.on_draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::linear_gradient(vec![
                            Color(0.15, 0.35, 0.85, 1.0),
                            Color(0.08, 0.2, 0.55, 1.0),
                        ]),
                        CornerRadii::uniform(16.0),
                    );
                });
            })
            .padding(12.0),
        ButtonSpec::default(),
        {
            move || {
                async_message.set("Fetching value on background thread...".to_string());
                fetch_request.update(|value| *value += 1);
            }
        },
        || {
            Text(
                "Fetch async value",
                Modifier::empty().padding(6.0),
                TextStyle::default(),
            );
        },
    );
}
