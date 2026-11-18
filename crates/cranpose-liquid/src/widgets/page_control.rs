use std::rc::Rc;

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Point, SemanticsConfiguration, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color};

// The iOS 27 page control, measured on the simulator at 3x: dots 23/3 points
// across, 53/3 points apart center to center, with the touch area 28 points
// tall. The current dot is white and the others white at 45%, made for the
// photos and dark screens a pager usually shows.
const DOT: f32 = 23.0 / 3.0;
const PITCH: f32 = 53.0 / 3.0;
const HEIGHT: f32 = 28.0;
const OTHER_ALPHA: f32 = 0.45;

/// A row of dots that shows which of `page_count` pages is `current_page`.
/// A tap on the left of the current dot turns back a page and on its right
/// turns forward one; `on_page_change` receives the requested page. Readers
/// hear "page N of M" and can step it like a slider.
///
/// UIKit's `UIPageControl`, the indicator of SwiftUI's `TabView(.page)`.
#[composable]
pub fn LiquidPageControl(
    modifier: Modifier,
    page_count: usize,
    current_page: usize,
    on_page_change: impl Fn(usize) + 'static,
) {
    let current = current_page.min(page_count.saturating_sub(1));
    let width = PITCH * page_count.saturating_sub(1) as f32 + DOT;
    let on_page_change: Rc<dyn Fn(usize)> = Rc::new(on_page_change);
    let step = Rc::clone(&on_page_change);
    let last = page_count.saturating_sub(1);
    let center_x = DOT * 0.5 + PITCH * current as f32;
    Box(
        modifier
            .size(Size::new(width, HEIGHT))
            .clickable(move |at: Point| {
                let next = if at.x < center_x {
                    current.saturating_sub(1)
                } else {
                    (current + 1).min(last)
                };
                if next != current {
                    step(next);
                }
            })
            .semantics(move |config: &mut SemanticsConfiguration| {
                config.state_description = Some(format!("{} / {}", current + 1, page_count));
                config.progress = Some(cranpose_ui::ProgressBarRangeInfo::new(
                    current as f32,
                    0.0,
                    last as f32,
                    u32::try_from(last.saturating_sub(1)).unwrap_or(u32::MAX),
                ));
                let on_page_change = Rc::clone(&on_page_change);
                config.set_progress =
                    Some(cranpose_ui::SemanticsSetProgress::new(move |page: f32| {
                        let page = nearest_page(page, last);
                        if page != current {
                            on_page_change(page);
                        }
                        true
                    }));
            })
            .draw_behind(move |scope| {
                for page in 0..page_count {
                    let alpha = if page == current { 1.0 } else { OTHER_ALPHA };
                    scope.draw_circle(
                        Brush::solid(Color::WHITE.with_alpha(alpha)),
                        Point::new(DOT * 0.5 + PITCH * page as f32, HEIGHT * 0.5),
                        DOT * 0.5,
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

// The page nearest to `page`, from 0 to `last`.
fn nearest_page(page: f32, last: usize) -> usize {
    (1..=last)
        .take_while(|&next| next as f32 - 0.5 <= page)
        .count()
}
