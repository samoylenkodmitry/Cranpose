use super::*;

#[test]
fn fallback_text_metrics_cover_empty_and_multiline_text() {
    let empty = fallback_text_metrics("", 10.0);
    assert_eq!(empty.line_count, 1);
    assert_eq!(empty.width, 0.0);
    assert_eq!(empty.height, fallback_line_height(10.0));

    let multiline = fallback_text_metrics("ab\ncde", 10.0);
    assert_eq!(multiline.line_count, 2);
    assert_eq!(multiline.width, 3.0 * fallback_char_width(10.0));
    assert_eq!(multiline.height, 2.0 * fallback_line_height(10.0));
}

#[test]
fn fallback_cursor_position_handles_non_boundary_byte_offsets() {
    let text = "éx";
    let width = fallback_char_width(12.0);
    assert_eq!(fallback_cursor_x_for_byte_offset(text, 0, 12.0), 0.0);
    assert_eq!(fallback_cursor_x_for_byte_offset(text, 1, 12.0), width);
    assert_eq!(
        fallback_cursor_x_for_byte_offset(text, text.len(), 12.0),
        width * 2.0
    );
}
