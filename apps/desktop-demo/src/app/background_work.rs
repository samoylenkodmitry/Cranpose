use cranpose_core::LaunchedEffect;
use cranpose_ui::{
    composable, worker_job, Brush, Button, ButtonSpec, Color, CornerRadii, Modifier, Spacer, Text,
    TextStyle,
};

use super::demo_random_u32;

// What the line says while a fetch runs. A browser page has one thread, and
// `launch_background` runs its work there.
const FETCHING: &str = if cfg!(target_arch = "wasm32") {
    "Fetching value..."
} else {
    "Fetching value on background thread..."
};

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
                async_message.set(FETCHING.to_string());
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

    Spacer(Modifier::empty().size_points(0.0, 12.0));
    CountPrimes();
}

const PRIME_LIMIT: u32 = 20_000_000;

// How many primes lie below `limit`: a sieve that takes long enough to hold a
// frame back where it runs on the UI thread.
#[worker_job]
fn count_primes(limit: u32) -> u32 {
    let limit = limit as usize;
    let mut composite = vec![false; limit.max(2)];
    let mut count = 0;
    for number in 2..limit {
        if composite[number] {
            continue;
        }
        count += 1;
        // A square past the limit, or past `usize` on a 32-bit target, has
        // no multiple to mark.
        let Some(square) = number.checked_mul(number).filter(|square| *square < limit) else {
            continue;
        };
        for multiple in (square..limit).step_by(number) {
            composite[multiple] = true;
        }
    }
    count
}

// A line with the last count over the button that starts the next: the count
// runs on a thread, and in a browser in a Web Worker.
#[composable]
fn CountPrimes() {
    let message = cranpose_core::rememberMutableStateOf(|| {
        "Tap \"Count primes in a worker\" to compute off the UI thread".to_string()
    });
    let request = cranpose_core::rememberMutableStateOf(|| 0u64);
    let key = request.get();
    LaunchedEffect(key, async move |_scope| {
        if key == 0 {
            return;
        }
        match cranpose_core::withBlocking(count_primes.with(PRIME_LIMIT)).await {
            Ok(count) => {
                message.set(format!(
                    "Worker job #{key}: {count} primes below {PRIME_LIMIT}"
                ));
            }
            Err(error) => message.set(format!("Worker job #{key} failed: {error}")),
        }
    });

    Text(
        message.get(),
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
            .background(Color(0.15, 0.35, 0.85, 1.0))
            .padding(12.0),
        ButtonSpec::default(),
        move || {
            message.set("Counting primes in a worker...".to_string());
            request.update(|value| *value += 1);
        },
        || {
            Text(
                "Count primes in a worker",
                Modifier::empty().padding(6.0),
                TextStyle::default(),
            );
        },
    );
}
