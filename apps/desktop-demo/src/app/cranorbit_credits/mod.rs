//! A round-watch Credits list, as a frame-pacing fixture.
//!
//! This is the heaviest text page from a Wear OS app built on Cranpose: ten
//! rows of wrapped paragraphs on a 454dp round display, each row re-scaled and
//! re-faded by a scaling-list curve as it travels past the centre line. It is
//! here because the demo had no page like it — the other tabs are widget trees,
//! and this is a single `Canvas` redrawing several hundred glyphs per frame,
//! which is a different shape of load and the one that showed up as slow.
//!
//! The layout code below is a **copy**, not a dependency: `draw.rs`, `font.rs`,
//! `layout.rs`, `theme.rs` and `list.rs` are vendored from the app so the demo
//! stays self-contained and the framework depends on nothing outside itself.
//! They are carried whole rather than filleted down to the few paths Credits
//! touches, so the fixture keeps drawing what the real screen draws; the unused
//! remainder is what `allow(dead_code)` covers.
//!
//! The list scrolls on its own, by elapsed time rather than per frame. Anything
//! paced per frame travels faster the faster the app runs, which would make a
//! frame-rate reading partly a measurement of itself.

#![allow(dead_code)]

mod draw;
mod font;
mod layout;
mod list;
mod rows;
mod theme;

use cranpose_core::{remember, useState, LaunchedEffectAsync};
use cranpose_ui::{Alignment, Box as CranposeBox, BoxSpec, Canvas, Modifier, Size};
use cranpose_ui_graphics::DrawScope;
use draw::Painter;
use font::Typography;
use list::ListStyle;
use std::cell::RefCell;
use std::rc::Rc;
use theme::ColorScheme;

/// The display the screen is laid out for. Drawing it at another size measures
/// a different page than the one that ships.
const WATCH_DP: f32 = 454.0;
/// The Credits screen's own side padding.
const SIDE_DP: f32 = 30.0;
/// Rows per second the list travels, so the workload is identical at every
/// pacing mode.
const ROWS_PER_SECOND: f32 = 5.0;

/// The `count_primitive` hook the vendored painter calls. The app uses it to
/// feed a performance overlay; here the frame counter already lives in the
/// dev overlay, so it costs nothing.
fn count_primitive() {}

struct CreditsState {
    rows: Vec<rows::SettingsRow>,
    slots: Vec<layout::Slot>,
    scroll: f32,
    down: bool,
    last_nanos: u64,
}

impl CreditsState {
    fn new() -> Self {
        Self {
            rows: rows::credits_rows(),
            slots: Vec::new(),
            scroll: 0.0,
            down: true,
            last_nanos: 0,
        }
    }

    /// Walks the list up and down by elapsed time, reversing at both ends so a
    /// measurement window of any length stays in motion.
    fn advance(&mut self, now_nanos: u64) {
        let last = std::mem::replace(&mut self.last_nanos, now_nanos);
        if last == 0 || now_nanos <= last {
            return;
        }
        let seconds = ((now_nanos - last) as f32 / 1_000_000_000.0).min(0.25);
        let limit = (self.rows.len() - 1) as f32;
        if self.down && self.scroll >= limit {
            self.down = false;
        } else if !self.down && self.scroll <= 0.0 {
            self.down = true;
        }
        let step = ROWS_PER_SECOND * seconds;
        self.scroll = (self.scroll + if self.down { step } else { -step }).clamp(0.0, limit);
    }
}

#[cranpose::composable]
pub fn cranorbit_credits_tab() {
    let state = remember(|| Rc::new(RefCell::new(CreditsState::new()))).with(|value| value.clone());
    let frame = useState(|| 0u64);

    let tick_state = state.clone();
    let tick_frame = frame;
    LaunchedEffectAsync!((), move |scope| {
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            loop {
                if !scope.is_active() {
                    break;
                }
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                tick_state.borrow_mut().advance(now);
                tick_frame.set(now);
            }
        })
    });

    let draw_state = state.clone();
    let draw_frame = frame;

    CranposeBox(
        Modifier::empty().fill_max_size(),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            let draw_state = draw_state.clone();
            let draw_frame = draw_frame;
            Canvas(
                Modifier::empty().size(Size::new(WATCH_DP, WATCH_DP)),
                move |scope: &mut dyn DrawScope| {
                    let _ = draw_frame.get();
                    let mut state = draw_state.borrow_mut();
                    let size = scope.size();
                    let width = size.width;
                    let height = size.height;
                    let radius = width.min(height) * 0.5;
                    let palette = theme::default_palette();
                    let style = ListStyle::new(Typography::new(2.0), SIDE_DP, width, height);
                    let scroll = state.scroll;

                    let mut painter = Painter::new(scope, width * 0.5, height * 0.5, radius);
                    let CreditsState { rows, slots, .. } = &mut *state;
                    list::layout_into(&painter, rows, &style, scroll, slots);
                    list::draw_list(&mut painter, rows, &style, slots, &palette);
                    if let Some(geometry) =
                        list::indicator_geometry(slots, style.height, style.gap())
                    {
                        list::draw_scroll_indicator(
                            &mut painter,
                            geometry,
                            1.0,
                            &ColorScheme::of(&palette),
                        );
                    }
                },
            );
        },
    );
}
