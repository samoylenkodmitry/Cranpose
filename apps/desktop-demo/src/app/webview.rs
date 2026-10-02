use cranpose::prelude::*;

fn control() -> Modifier {
    Modifier::empty()
        .height(48.0)
        .background(Color(0.2, 0.45, 0.9, 1.0))
        .rounded_corners(8.0)
        .padding(8.0)
}

#[composable]
pub(crate) fn WebViewTab() {
    let count = rememberMutableStateOf(|| 0);
    let visible = rememberMutableStateOf(|| true);
    let status = rememberMutableStateOf(|| String::from("Loading https://example.com…"));
    Column(
        Modifier::empty()
            .fill_max_size()
            .padding_each(12.0, 12.0, 12.0, 60.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(10.0)),
        move || {
            Text(
                "A website inside Cranpose",
                Modifier::empty(),
                TextStyle::default(),
            );
            Text(status.value(), Modifier::empty(), TextStyle::default());
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                move || {
                    Button(
                        control(),
                        ButtonSpec::default(),
                        move || count.set(count.value() + 1),
                        move || {
                            Text(
                                format!("Cranpose: {}", count.value()),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                    Button(
                        control(),
                        ButtonSpec::default(),
                        move || {
                            visible.set(!visible.value());
                            if visible.value() {
                                status.set(String::from("Loading https://example.com…"));
                            }
                        },
                        move || {
                            Text(
                                if visible.value() {
                                    "Remove WebView"
                                } else {
                                    "Show WebView"
                                },
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                },
            );
            if visible.value() {
                WebView(
                    "https://example.com",
                    Modifier::empty().fill_max_width().weight(1.0),
                    move |event| {
                        status.set(match event {
                            WebViewEvent::Loaded(url) => format!("Loaded {url}"),
                            WebViewEvent::Failed(message) => format!("WebView: {message}"),
                        });
                    },
                );
            }
        },
    );
}
