use cranpose::{
    composable, rememberMutableStateOf, Alignment, Box, BoxSpec, Color, Modifier, SpanStyle, Text,
    TextStyle,
};

#[allow(non_snake_case)]
#[composable]
pub fn AccessibilityNavigationScreen() {
    let settings = rememberMutableStateOf(|| false);
    let selected = settings.get();
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(if selected { Color::GREEN } else { Color::BLUE })
            .clickable(move |_| settings.set(!settings.get())),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                if selected {
                    "Settings page"
                } else {
                    "Library page"
                },
                Modifier::empty(),
                TextStyle::from_span_style(SpanStyle {
                    color: Some(Color::WHITE),
                    ..SpanStyle::default()
                }),
            );
        },
    );
}
