type CanvasStyle = (&'static str, &'static str);

const VIEWPORT_FILLING_CANVAS_STYLES: [CanvasStyle; 7] = [
    ("width", "100vw"),
    ("height", "100vh"),
    ("height", "100dvh"),
    ("touch-action", "none"),
    ("user-select", "none"),
    ("-webkit-user-select", "none"),
    ("-webkit-touch-callout", "none"),
];

// A long press belongs to the app: the browser starts no text selection on
// the canvas and shows no callout over it. WebKit reads the prefixed names.
const PAGE_LAID_OUT_CANVAS_STYLES: [CanvasStyle; 4] = [
    ("touch-action", "none"),
    ("user-select", "none"),
    ("-webkit-user-select", "none"),
    ("-webkit-touch-callout", "none"),
];

// The accessibility mirror's root: over the whole canvas, taking no pointer.
// Its transparent text lies where a finger presses, so it is unselectable
// too, and its nodes take that from it.
pub(crate) const MIRROR_ROOT_STYLES: [CanvasStyle; 6] = [
    ("position", "fixed"),
    ("inset", "0"),
    ("z-index", "2147483647"),
    ("pointer-events", "none"),
    ("user-select", "none"),
    ("-webkit-user-select", "none"),
];

pub(crate) fn canvas_inline_styles(fill_viewport: bool) -> &'static [CanvasStyle] {
    if fill_viewport {
        &VIEWPORT_FILLING_CANVAS_STYLES
    } else {
        &PAGE_LAID_OUT_CANVAS_STYLES
    }
}

#[cfg(test)]
#[path = "tests/canvas_layout_tests.rs"]
mod tests;
