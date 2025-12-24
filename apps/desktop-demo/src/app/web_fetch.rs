#[cfg(target_arch = "wasm32")]
use compose_core::LaunchedEffectAsync;
use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier,
    Row, RowSpec, Size, Spacer, Text, VerticalAlignment,
};

#[cfg(not(target_arch = "wasm32"))]
use compose_core::LaunchedEffect;

#[derive(Clone, Debug, PartialEq, Eq)]
enum FetchStatus {
    Idle,
    Loading,
    Success(String),
    Error(String),
}

/// Performs HTTP fetch - native implementation using reqwest blocking client
#[cfg(not(target_arch = "wasm32"))]
fn do_fetch_blocking() -> Result<String, String> {
    use reqwest::blocking::Client;

    let client = Client::builder()
        .user_agent("compose-rs-desktop-demo/0.1")
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    // Use ipify.org - simple, reliable, CORS-friendly
    let response = client
        .get("https://api.ipify.org?format=json")
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .map_err(|e| format!("Failed to read body: {}", e))?;

    if status.is_success() {
        // Parse JSON response to extract IP address
        if let Some(start) = body.find("\"ip\"") {
            if let Some(colon) = body[start..].find(':') {
                let after_colon = &body[start + colon + 1..];
                if let Some(quote_start) = after_colon.find('"') {
                    if let Some(quote_end) = after_colon[quote_start + 1..].find('"') {
                        let ip = &after_colon[quote_start + 1..quote_start + 1 + quote_end];
                        return Ok(format!("Your public IP: {}", ip));
                    }
                }
            }
        }
        Ok(body.trim().to_string())
    } else {
        Err(format!("Request failed with status {}: {}", status, body))
    }
}

/// Performs HTTP fetch - WASM implementation using browser's fetch API
#[cfg(target_arch = "wasm32")]
async fn do_fetch_async() -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, RequestMode, Response};

    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

    // Use ipify.org - simple, reliable, CORS-friendly
    let request = Request::new_with_str_and_init("https://api.ipify.org?format=json", &opts)
        .map_err(|e| format!("Failed to create request: {:?}", e))?;

    let window = web_sys::window().ok_or("No window object")?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("Fetch failed: {:?}", e))?;

    let resp: Response = resp_value
        .dyn_into()
        .map_err(|_| "Response is not a Response object")?;

    if !resp.ok() {
        return Err(format!("Request failed with status {}", resp.status()));
    }

    let text_promise = resp
        .text()
        .map_err(|e| format!("Failed to get text: {:?}", e))?;
    let text_value = JsFuture::from(text_promise)
        .await
        .map_err(|e| format!("Failed to read body: {:?}", e))?;

    // Parse JSON response to extract IP address
    let text = text_value
        .as_string()
        .ok_or_else(|| "Response body is not a string".to_string())?;

    // Extract IP from {"ip": "..."} response
    if let Some(start) = text.find("\"ip\"") {
        if let Some(colon) = text[start..].find(':') {
            let after_colon = &text[start + colon + 1..];
            if let Some(quote_start) = after_colon.find('"') {
                if let Some(quote_end) = after_colon[quote_start + 1..].find('"') {
                    let ip = &after_colon[quote_start + 1..quote_start + 1 + quote_end];
                    return Ok(format!("Your public IP: {}", ip));
                }
            }
        }
    }

    Ok(text.trim().to_string())
}

#[composable]
pub(crate) fn web_fetch_example() {
    let fetch_status = compose_core::useState(|| FetchStatus::Idle);
    let request_counter = compose_core::useState(|| 0u64);

    // Native implementation using blocking worker
    #[cfg(not(target_arch = "wasm32"))]
    {
        let status_state = fetch_status;
        let request_key = request_counter.get();
        LaunchedEffect!(request_key, move |scope| {
            if request_key == 0 {
                return;
            }

            let status = status_state;
            status.set(FetchStatus::Loading);

            scope.launch_background(
                move |token| {
                    if token.is_cancelled() {
                        return Err("request cancelled".to_string());
                    }
                    do_fetch_blocking()
                },
                move |fetch_result| match fetch_result {
                    Ok(text) => status.set(FetchStatus::Success(text)),
                    Err(error) => status.set(FetchStatus::Error(error)),
                },
            );
        });
    }

    // WASM implementation using async fetch
    #[cfg(target_arch = "wasm32")]
    {
        let status_state = fetch_status;
        let request_key = request_counter.get();
        LaunchedEffectAsync!(request_key, move |scope| {
            let status = status_state;
            Box::pin(async move {
                if request_key == 0 {
                    return;
                }

                status.set(FetchStatus::Loading);

                match do_fetch_async().await {
                    Ok(text) => {
                        if scope.is_active() {
                            status.set(FetchStatus::Success(text));
                        }
                    }
                    Err(error) => {
                        if scope.is_active() {
                            status.set(FetchStatus::Error(error));
                        }
                    }
                }
            })
        });
    }

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.12, 0.22, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        {
            let status_state = fetch_status;
            let request_state = request_counter;
            move || {
                Text(
                    "Fetch data from the web",
                    Modifier::empty()
                        .padding(12.0)
                        .background(Color(1.0, 1.0, 1.0, 0.08))
                        .rounded_corners(16.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                Text(
                    concat!(
                        "This tab uses LaunchedEffect to fetch your public IP address from ",
                        "api.ipify.org. Each click spawns an HTTP request and updates ",
                        "the UI when the response arrives.",
                    ),
                    Modifier::empty()
                        .padding(12.0)
                        .background(Color(0.12, 0.16, 0.28, 0.7))
                        .rounded_corners(14.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Row(
                    Modifier::empty().fill_max_width().padding(4.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let status_for_button = status_state;
                        let request_for_button = request_state;
                        move || {
                            Button(
                                Modifier::empty()
                                    .rounded_corners(14.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::linear_gradient(vec![
                                                Color(0.22, 0.52, 0.92, 1.0),
                                                Color(0.14, 0.42, 0.78, 1.0),
                                            ]),
                                            CornerRadii::uniform(14.0),
                                        );
                                    })
                                    .padding(10.0),
                                move || {
                                    status_for_button.set(FetchStatus::Loading);
                                    request_for_button.update(|tick| *tick = tick.wrapping_add(1));
                                },
                                || {
                                    Text(
                                        "Fetch motto",
                                        Modifier::empty()
                                            .padding(6.0)
                                            .background(Color(1.0, 1.0, 1.0, 0.05))
                                            .rounded_corners(10.0),
                                    );
                                },
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                let status_snapshot = status_state.get();
                let (status_label, banner_color) = match &status_snapshot {
                    FetchStatus::Idle => (
                        "Click the button to start an HTTP request",
                        Color(0.14, 0.24, 0.36, 0.8),
                    ),
                    FetchStatus::Loading => {
                        ("Contacting api.github.com...", Color(0.20, 0.30, 0.48, 0.9))
                    }
                    FetchStatus::Success(_) => {
                        ("Success: received response", Color(0.16, 0.42, 0.26, 0.85))
                    }
                    FetchStatus::Error(_) => ("Request failed", Color(0.45, 0.18, 0.18, 0.85)),
                };

                Text(
                    status_label,
                    Modifier::empty()
                        .padding(10.0)
                        .background(banner_color)
                        .rounded_corners(12.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 8.0,
                });

                match status_snapshot {
                    FetchStatus::Idle => {
                        Text(
                            "No request has been made yet.",
                            Modifier::empty()
                                .padding(10.0)
                                .background(Color(0.10, 0.16, 0.28, 0.7))
                                .rounded_corners(12.0),
                        );
                    }
                    FetchStatus::Loading => {
                        Text(
                            "Hang tight while the response arrives...",
                            Modifier::empty()
                                .padding(10.0)
                                .background(Color(0.12, 0.18, 0.32, 0.9))
                                .rounded_corners(12.0),
                        );
                    }
                    FetchStatus::Success(message) => {
                        Text(
                            format!("\"{}\"", message),
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.14, 0.34, 0.26, 0.9))
                                .rounded_corners(14.0),
                        );
                    }
                    FetchStatus::Error(error) => {
                        Text(
                            format!("Error: {}", error),
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.40, 0.18, 0.18, 0.9))
                                .rounded_corners(14.0),
                        );
                    }
                }
            }
        },
    );
}
