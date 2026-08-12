//! The cranorbit Credits screen, as a demo tab.
//!
//! This is not a reconstruction of that screen: it calls cranorbit's own
//! `screens::draw` with its own `AppState`, so the tab cannot drift from the
//! app it stands in for. What it adds is the harness — a 454dp square viewport
//! (the watch the screen is designed for) and a list that keeps scrolling, so
//! the page is never at rest and a frame-rate reading means something.
//!
//! Credits is the heaviest text page cranorbit has: ten rows of wrapped
//! paragraphs, every one of them re-scaled by the scaling-list curve as it
//! travels. That is what makes it the fixture worth measuring.

use cranorbit_app::app_state::{AppState, Screen};
use cranorbit_app::render::ArenaRenderer;
use cranorbit_app::ui::screens;
use cranpose_core::{remember, useState, LaunchedEffectAsync};
use cranpose_ui::{Alignment, Box as CranposeBox, BoxSpec, Canvas, Modifier, Size};
use cranpose_ui_graphics::DrawScope;
use std::cell::RefCell;
use std::rc::Rc;

/// The watch this screen is laid out for. Drawing it at any other size would
/// measure a different page than the one that ships.
const WATCH_DP: f32 = 454.0;

#[cranpose::composable]
pub fn cranorbit_credits_tab() {
    let app = remember(|| {
        let mut state = AppState::new();
        // Unlocked so the list carries every row, and auto-scrolling so the
        // page never settles -- cranorbit parks its frame loop when nothing
        // moves, which would otherwise read as 0fps.
        state.debug.unlocked = true;
        state.debug.autoscroll = true;
        state.goto(Screen::Credits);
        Rc::new(RefCell::new(state))
    })
    .with(|value| value.clone());
    let renderer = remember(|| Rc::new(RefCell::new(ArenaRenderer::new()))).with(|v| v.clone());
    let frame = useState(|| 0u64);

    let loop_app = app.clone();
    let loop_frame = frame.clone();
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
                loop_app.borrow_mut().tick(now);
                loop_frame.set(now);
            }
        })
    });

    let draw_frame = frame.clone();
    let draw_app = app.clone();
    let draw_renderer = renderer.clone();

    CranposeBox(
        Modifier::empty().fill_max_size(),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            let draw_frame = draw_frame.clone();
            let draw_app = draw_app.clone();
            let draw_renderer = draw_renderer.clone();
            Canvas(
                Modifier::empty().size(Size::new(WATCH_DP, WATCH_DP)),
                move |scope: &mut dyn DrawScope| {
                    let _ = draw_frame.get();
                    let mut state = draw_app.borrow_mut();
                    let mut renderer = draw_renderer.borrow_mut();
                    screens::draw(scope, &mut state, &mut renderer);
                },
            );
        },
    );
}
