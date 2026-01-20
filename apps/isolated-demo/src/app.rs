#![allow(non_snake_case)]

use cranpose::prelude::*;
use cranpose_core::useState;

const TITLE: &str = "Cranpose Isolated Demo";
const SUBTITLE: &str = "Published crates only";

pub(crate) fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title(TITLE)
        .with_size(900, 600)
        .with_fonts(crate::fonts::DEMO_FONTS)
        .with_fps_counter(true)
}

#[composable]
pub(crate) fn IsolatedDemoApp() {
    let counter = useState(|| 0i32);
    let accent = useState(|| false);

    let accent_color = if accent.value() {
        Color::rgba(0.18, 0.62, 0.42, 1.0)
    } else {
        Color::rgba(0.58, 0.24, 0.33, 1.0)
    };

    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(16.0)),
        move || {
            Text(TITLE, Modifier::empty());
            Text(SUBTITLE, Modifier::empty());

            Row(
                Modifier::empty(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    Button(
                        Modifier::empty()
                            .padding(12.0)
                            .background(Color::rgba(0.12, 0.16, 0.26, 1.0)),
                        {
                            let counter = counter;
                            move || counter.set(counter.get() + 1)
                        },
                        {
                            let counter = counter;
                            move || {
                                Text(format!("Count: {}", counter.value()), Modifier::empty());
                            }
                        },
                    );

                    Button(
                        Modifier::empty()
                            .padding(12.0)
                            .background(Color::rgba(0.2, 0.2, 0.2, 1.0)),
                        {
                            let accent = accent;
                            move || accent.set(!accent.get())
                        },
                        {
                            let accent = accent;
                            move || {
                                Text(
                                    if accent.value() {
                                        "Accent: On"
                                    } else {
                                        "Accent: Off"
                                    },
                                    Modifier::empty(),
                                );
                            }
                        },
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            Box(
                Modifier::empty()
                    .size(Size {
                        width: 320.0,
                        height: 120.0,
                    })
                    .background(accent_color),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    Text("Tap the accent button to swap themes", Modifier::empty());
                },
            );
        },
    );
}
