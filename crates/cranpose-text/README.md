# Cranpose Text

`cranpose-text` holds the values text is described with, as Compose's
`ui-text` does: `TextStyle` with its `SpanStyle` and `ParagraphStyle` parts,
`AnnotatedString` with spans, links and string annotations, font families,
weights and styles, text units, decorations and shadows, line boxes, and the
options that limit lines and choose the overflow behavior. Application code
reaches them through `cranpose::prelude` and `cranpose_ui::text`.

```rust
use cranpose_text::{AnnotatedString, FontWeight, SpanStyle, TextStyle, TextUnit};

let style = TextStyle {
    span_style: SpanStyle {
        font_size: TextUnit::Sp(16.0),
        font_weight: Some(FontWeight::BOLD),
        ..SpanStyle::default()
    },
    ..TextStyle::default()
};
let text = AnnotatedString::new("Hello".to_owned());
assert_eq!(text.text, "Hello");
let _ = style;
```
