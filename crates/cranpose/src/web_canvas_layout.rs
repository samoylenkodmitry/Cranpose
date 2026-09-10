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
mod tests {
    use super::canvas_inline_styles;

    fn properties(fill_viewport: bool) -> Vec<&'static str> {
        canvas_inline_styles(fill_viewport)
            .iter()
            .map(|(property, _)| *property)
            .collect()
    }

    #[test]
    fn page_laid_out_canvases_keep_the_box_their_container_gives_them() {
        assert_eq!(properties(false), vec!["touch-action"]);
    }

    #[test]
    fn viewport_filling_canvases_take_the_whole_viewport() {
        assert_eq!(
            canvas_inline_styles(true),
            [
                ("width", "100vw"),
                ("height", "100vh"),
                ("height", "100dvh"),
                ("touch-action", "none"),
            ]
        );
    }

    #[test]
    fn viewport_filling_heights_end_on_the_dynamic_unit() {
        let heights: Vec<&str> = canvas_inline_styles(true)
            .iter()
            .filter(|(property, _)| *property == "height")
            .map(|(_, value)| *value)
            .collect();
        assert_eq!(heights, vec!["100vh", "100dvh"]);
    }

    #[test]
    fn every_mode_takes_pointer_gestures_from_the_browser() {
        for fill_viewport in [false, true] {
            assert!(canvas_inline_styles(fill_viewport).contains(&("touch-action", "none")));
        }
    }
}
