use cranpose_services::{local_http_client, local_uri_handler, HttpClientRef};
use cranpose_ui::{
    composable,
    text::{SpanStyle, TextDecoration},
    Brush, Button, ButtonSpec, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier,
    Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum FetchStatus {
    Idle,
    Loading,
    Success(String),
    Error(String),
}

fn parse_ip_response(body: &str) -> String {
    if let Some(start) = body.find("\"ip\"") {
        if let Some(colon) = body[start..].find(':') {
            let after_colon = &body[start + colon + 1..];
            if let Some(quote_start) = after_colon.find('"') {
                if let Some(quote_end) = after_colon[quote_start + 1..].find('"') {
                    let ip = &after_colon[quote_start + 1..quote_start + 1 + quote_end];
                    return format!("Your public IP: {}", ip);
                }
            }
        }
    }
    body.trim().to_string()
}

async fn fetch_ipify(client: &HttpClientRef) -> Result<String, String> {
    let body = client
        .get_text("https://api.ipify.org?format=json")
        .await
        .map_err(|err| format!("Request failed: {}", err))?;
    Ok(parse_ip_response(&body))
}

#[composable]
pub(crate) fn web_fetch_example() {
    let fetch_status = cranpose_core::rememberMutableStateOf(|| FetchStatus::Idle);
    let request_counter = cranpose_core::rememberMutableStateOf(|| 0u64);
    let uri_handler = local_uri_handler().current();
    let http_client = local_http_client().current();

    cranpose_core::LaunchedEffect!(request_counter.get(), move |scope| {
        let request_key = request_counter.get();
        if request_key == 0 {
            return;
        }

        fetch_status.set(FetchStatus::Loading);

        let client = http_client.clone();
        scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return Err("request cancelled".to_string());
                }
                fetch_ipify(&client).await
            },
            move |fetch_result| match fetch_result {
                Ok(text) => fetch_status.set(FetchStatus::Success(text)),
                Err(error) => fetch_status.set(FetchStatus::Error(error)),
            },
        );
    });

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.12, 0.22, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        {
            let uri_handler = uri_handler.clone();
            move || {
                let uri_handler = uri_handler.clone();
                Text(
                    "Fetch data from the web",
                    Modifier::empty()
                        .padding(12.0)
                        .background(Color(1.0, 1.0, 1.0, 0.08))
                        .rounded_corners(16.0),
                    TextStyle::default(),
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
                    TextStyle::default(),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                let api_url = "https://api.ipify.org";
                let link_handler = uri_handler.clone();
                Row(
                    Modifier::empty().fill_max_width().padding(4.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    move || {
                        let link_handler = link_handler.clone();
                        Text(
                            "API Endpoint:",
                            Modifier::empty().padding(2.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.7, 0.74, 0.86, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        Text(
                            api_url,
                            Modifier::empty().padding(2.0).clickable(move |_| {
                                if let Err(err) = link_handler.open_uri(api_url) {
                                    log::error!("Failed to open {}: {:#}", api_url, err);
                                }
                            }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.32, 0.72, 0.98, 1.0)),
                                    text_decoration: Some(TextDecoration::UNDERLINE),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                Row(
                    Modifier::empty().fill_max_width().padding(4.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
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
                                ButtonSpec::default(),
                                move || {
                                    fetch_status.set(FetchStatus::Loading);
                                    request_counter.update(|tick| *tick = tick.wrapping_add(1));
                                },
                                || {
                                    Text(
                                        "Fetch motto",
                                        Modifier::empty()
                                            .padding(6.0)
                                            .background(Color(1.0, 1.0, 1.0, 0.05))
                                            .rounded_corners(10.0),
                                        TextStyle::default(),
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

                let status_snapshot = fetch_status.get();
                let (status_label, banner_color) = match &status_snapshot {
                    FetchStatus::Idle => (
                        "Click the button to start an HTTP request",
                        Color(0.14, 0.24, 0.36, 0.8),
                    ),
                    FetchStatus::Loading => {
                        ("Contacting api.ipify.org...", Color(0.20, 0.30, 0.48, 0.9))
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
                    TextStyle::default(),
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
                            TextStyle::default(),
                        );
                    }
                    FetchStatus::Loading => {
                        Text(
                            "Hang tight while the response arrives...",
                            Modifier::empty()
                                .padding(10.0)
                                .background(Color(0.12, 0.18, 0.32, 0.9))
                                .rounded_corners(12.0),
                            TextStyle::default(),
                        );
                    }
                    FetchStatus::Success(message) => {
                        Text(
                            format!("\"{}\"", message),
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.14, 0.34, 0.26, 0.9))
                                .rounded_corners(14.0),
                            TextStyle::default(),
                        );
                    }
                    FetchStatus::Error(error) => {
                        Text(
                            format!("Error: {}", error),
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.40, 0.18, 0.18, 0.9))
                                .rounded_corners(14.0),
                            TextStyle::default(),
                        );
                    }
                }
            }
        },
    );
}
