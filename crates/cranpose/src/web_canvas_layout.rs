type CanvasStyle = (&'static str, &'static str);

const VIEWPORT_FILLING_CANVAS_STYLES: [CanvasStyle; 4] = [
    ("width", "100vw"),
    ("height", "100vh"),
    ("height", "100dvh"),
    ("touch-action", "none"),
];

const PAGE_LAID_OUT_CANVAS_STYLES: [CanvasStyle; 1] = [("touch-action", "none")];

pub(crate) fn canvas_inline_styles(fill_viewport: bool) -> &'static [CanvasStyle] {
    if fill_viewport {
        &VIEWPORT_FILLING_CANVAS_STYLES
    } else {
        &PAGE_LAID_OUT_CANVAS_STYLES
    }
}

#[cfg(test)]
#[path = "tests/web_canvas_layout_tests.rs"]
mod tests;
