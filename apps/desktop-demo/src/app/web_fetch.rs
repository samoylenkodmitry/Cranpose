use anyhow::{anyhow, Context};
use compose_core::LaunchedEffect;
use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier,
    Row, RowSpec, Size, Spacer, Text, VerticalAlignment,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum FetchStatus {
    Idle,
    Loading,
    Success(String),
    Error(String),
}

#[composable]
pub(crate) fn web_fetch_example() {
    let fetch_status = compose_core::useState(|| FetchStatus::Idle);
    let request_counter = compose_core::useState(|| 0u64);

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
                        return Err(anyhow!("request cancelled"));
                    }

                    let client = reqwest::blocking::Client::builder()
                        .user_agent("compose-rs-desktop-demo/0.1")
                        .build()
                        .context("building HTTP client")?;

                    let response = client
                        .get("https://api.github.com/zen")
                        .send()
                        .context("sending request")?;

                    let status = response.status();
                    let body = response.text().context("reading response body")?;

                    if status.is_success() {
                        Ok(body.trim().to_string())
                    } else {
                        Err(anyhow!("Request failed with status {}: {}", status, body))
                    }
                },
                move |fetch_result| match fetch_result {
                    Ok(text) => status.set(FetchStatus::Success(text)),
                    Err(error) => status.set(FetchStatus::Error(error.to_string())),
                },
            );
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
                        "This tab uses LaunchedEffect with a background worker to ",
                        "request a short motto from api.github.com/zen. Each click ",
                        "spawns a request and updates the UI when the response ",
                        "arrives.",
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
