//! The display options a browser reports: the reduced motion, reduced
//! transparency and more contrast media queries, and the root font size a
//! person raised in the browser's settings.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_render_common::Renderer;
use cranpose_services::AccessibilityOptions;
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::{MediaQueryList, Window};

const QUERIES: [&str; 3] = [
    "(prefers-reduced-motion: reduce)",
    "(prefers-reduced-transparency: reduce)",
    "(prefers-contrast: more)",
];

/// The browser's default root font size in CSS pixels.
const DEFAULT_ROOT_FONT_PX: f32 = 16.0;

/// Reads the options once and again on every change of a media query, and
/// installs them on the shell with a root render and a frame.
pub(crate) fn watch<R>(window: &Window, app: Rc<RefCell<AppShell<R>>>, request_frame: Rc<dyn Fn()>)
where
    R: Renderer + 'static,
    R::Error: std::fmt::Debug,
{
    let queries: Vec<MediaQueryList> = QUERIES
        .iter()
        .filter_map(|query| window.match_media(query).ok().flatten())
        .collect();
    let font_scale = root_font_scale(window);
    let queries = Rc::new(queries);
    apply(&app, &queries, font_scale);
    for query in queries.iter() {
        let app = Rc::clone(&app);
        let queries = Rc::clone(&queries);
        let request_frame = Rc::clone(&request_frame);
        let closure = Closure::wrap(Box::new(move |_event: web_sys::MediaQueryListEvent| {
            if apply(&app, &queries, font_scale) {
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        if query
            .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
            .is_err()
        {
            log::debug!("the browser took no listener for an accessibility media query");
        }
        closure.forget();
    }
}

fn apply<R>(app: &Rc<RefCell<AppShell<R>>>, queries: &[MediaQueryList], font_scale: f32) -> bool
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    let matches = |index: usize| queries.get(index).is_some_and(MediaQueryList::matches);
    let options = AccessibilityOptions {
        font_scale,
        reduce_motion: matches(0),
        reduce_transparency: matches(1),
        increase_contrast: matches(2),
        ..AccessibilityOptions::default()
    };
    let mut shell = app.borrow_mut();
    let changed = crate::accessibility::apply_accessibility_options(&mut shell, options);
    if changed {
        shell.set_font_scale(options.font_scale);
    }
    changed
}

/// The root font size over the browser default: what a person who raised the
/// font size in the browser's settings gets, as page text does.
fn root_font_scale(window: &Window) -> f32 {
    window
        .document()
        .and_then(|document| document.document_element())
        .and_then(|root| window.get_computed_style(&root).ok().flatten())
        .and_then(|style| style.get_property_value("font-size").ok())
        .map_or(1.0, |size| font_scale_from_css(&size))
}

/// The scale a `font-size` value such as `20px` stands for.
pub(crate) fn font_scale_from_css(size: &str) -> f32 {
    size.trim()
        .strip_suffix("px")
        .and_then(|px| px.trim().parse::<f32>().ok())
        .filter(|px| px.is_finite() && *px > 0.0)
        .map_or(1.0, |px| px / DEFAULT_ROOT_FONT_PX)
}

#[cfg(test)]
mod tests {
    use super::font_scale_from_css;

    #[test]
    fn a_larger_root_font_reads_as_a_scale_and_junk_reads_as_one() {
        assert_eq!(font_scale_from_css("16px"), 1.0);
        assert_eq!(font_scale_from_css("20px"), 1.25);
        assert_eq!(font_scale_from_css(" 24px "), 1.5);
        assert_eq!(font_scale_from_css(""), 1.0);
        assert_eq!(font_scale_from_css("large"), 1.0);
        assert_eq!(font_scale_from_css("0px"), 1.0);
    }
}
