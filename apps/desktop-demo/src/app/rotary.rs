use cranpose_core::rememberMutableStateOf;
use cranpose_ui::{
    composable, text::TextUnit, Color, Column, ColumnSpec, Modifier, Text, TextStyle,
};

fn heading_style(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style
}

#[composable]
pub fn rotary_tab() {
    let accumulated = rememberMutableStateOf(|| 0.0f32);
    let last_delta = rememberMutableStateOf(|| 0.0f32);
    let events = rememberMutableStateOf(|| 0u32);
    let pre_events = rememberMutableStateOf(|| 0u32);

    let pre_counter = pre_events;
    let modifier = Modifier::empty()
        .fill_max_size()
        .on_pre_rotary_scroll_event(move |_event| {
            pre_counter.set(pre_counter.get() + 1);
            false
        })
        .on_rotary_scroll_event(move |event| {
            accumulated.set(accumulated.get() + event.vertical_scroll_pixels);
            last_delta.set(event.vertical_scroll_pixels);
            events.set(events.get() + 1);
            true
        });

    Column(modifier.padding(24.0), ColumnSpec::default(), move || {
        Text(
            "Rotary input demo (crown / bezel)",
            Modifier::empty().padding(6.0),
            heading_style(24.0, Color(0.95, 0.95, 1.0, 1.0)),
        );
        Text(
            "Scroll the mouse wheel - it stands in for the watch crown.",
            Modifier::empty().padding(6.0),
            heading_style(14.0, Color(0.70, 0.70, 0.82, 1.0)),
        );
        Text(
            format!("accumulated: {:.1} px", accumulated.get()),
            Modifier::empty().padding(6.0),
            heading_style(32.0, Color(0.40, 0.90, 0.60, 1.0)),
        );
        Text(
            format!("last vertical_scroll_pixels: {:.1}", last_delta.get()),
            Modifier::empty().padding(6.0),
            heading_style(16.0, Color(0.85, 0.85, 0.92, 1.0)),
        );
        Text(
            format!(
                "bubble events: {}   capture events: {}",
                events.get(),
                pre_events.get()
            ),
            Modifier::empty().padding(6.0),
            heading_style(16.0, Color(0.85, 0.85, 0.92, 1.0)),
        );
        Text(
            "Wheel up produces a NEGATIVE delta (Compose convention).",
            Modifier::empty().padding(6.0),
            heading_style(13.0, Color(0.60, 0.60, 0.72, 1.0)),
        );
    });
}
