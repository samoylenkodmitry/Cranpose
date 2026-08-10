//! Rotary input demo (Wear OS crown / rotating bezel).
//!
//! On a watch this screen is driven by the Pixel Watch crown or the Galaxy
//! Watch rotating bezel. On the desktop the mouse wheel stands in for the
//! crown: `dispatch_mouse_wheel` translates every `WindowEvent::MouseWheel`
//! into a `RotaryScrollEvent` and offers it to rotary handlers before falling
//! back to ordinary scrolling. Scrolling the wheel anywhere over this screen
//! moves the value below.
//!
//! Scroll the wheel **up** and the accumulated value goes **down**: a positive
//! Android `AXIS_SCROLL` detent maps to a negative `vertical_scroll_pixels`,
//! which is Compose's convention.

use cranpose_core::useState;
use cranpose_ui::{
    composable, text::TextUnit, Color, Column, ColumnSpec, Modifier, Text, TextStyle,
};

fn heading_style(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style
}

/// Demo screen showing raw rotary deltas and an accumulated value.
///
/// Demonstrates both phases of the Compose rotary API:
/// `on_pre_rotary_scroll_event` (capture) and `on_rotary_scroll_event`
/// (bubble).
#[composable]
pub fn rotary_tab() {
    let accumulated = useState(|| 0.0f32);
    let last_delta = useState(|| 0.0f32);
    let events = useState(|| 0u32);
    let pre_events = useState(|| 0u32);

    // Capture (pre) pass, root -> focused. Returning `false` lets the event
    // continue on to the bubble handler below; returning `true` would consume
    // it and the bubble handler would never run.
    let pre_counter = pre_events;
    let modifier = Modifier::empty()
        .fill_max_size()
        .on_pre_rotary_scroll_event(move |_event| {
            pre_counter.set(pre_counter.get() + 1);
            false
        })
        // Bubble pass, focused -> root: the normal place to handle rotary.
        .on_rotary_scroll_event(move |event| {
            accumulated.set(accumulated.get() + event.vertical_scroll_pixels);
            last_delta.set(event.vertical_scroll_pixels);
            events.set(events.get() + 1);
            // Consume, so the wheel does not additionally scroll a parent.
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
