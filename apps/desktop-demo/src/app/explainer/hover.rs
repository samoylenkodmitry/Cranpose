// A pointer over a stage, and how long it has been there: its presence
// comes and goes by the clock, not by frames, so a slow machine settles as
// soon. The melt warms the chocolate under it; the lamp leans toward it.

use cranpose_core::{derivedStateOf, rememberMutableStateOf, MutableState};
use cranpose_ui::{composable, Modifier, Point, PointerEventKind, PointerInputScope};

use crate::app::ticking::Ticking;

// A second.
const COMING: f32 = 5.0;
const GOING: f32 = 3.0;
const SETTLED: f32 = 0.012;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Spot {
    over: bool,
    at: Point,
    strength: f32,
}

#[derive(Clone, Copy)]
pub(super) struct Hover(MutableState<Spot>);

impl Hover {
    pub(super) fn now(self) -> (Point, f32) {
        let spot = self.0.get();
        (spot.at, spot.strength)
    }

    pub(super) fn follow(self, modifier: Modifier) -> Modifier {
        let spot = self.0;
        modifier.pointer_input((), move |scope: PointerInputScope| async move {
            scope
                .await_pointer_event_scope(|events| async move {
                    loop {
                        let event = events.await_pointer_event().await;
                        let over = match event.kind {
                            PointerEventKind::Enter
                            | PointerEventKind::Move
                            | PointerEventKind::Down => true,
                            PointerEventKind::Exit | PointerEventKind::Cancel => false,
                            _ => continue,
                        };
                        let mut next = spot.get_non_reactive();
                        next.over = over;
                        if over {
                            next.at = event.position;
                        }
                        spot.set(next);
                    }
                })
                .await;
        })
    }
}

#[composable(no_skip)]
pub(super) fn rememberHover(moving: bool) -> Hover {
    let spot = rememberMutableStateOf(Spot::default);
    let easing = derivedStateOf(move || {
        let now = spot.get();
        (f32::from(u8::from(now.over)) - now.strength).abs() > 0.0
    })
    .get();
    Ticking(moving && easing, move |seconds| {
        let mut now = spot.get_non_reactive();
        let wanted = f32::from(u8::from(now.over));
        let rate = if now.over { COMING } else { GOING };
        now.strength += (wanted - now.strength) * (1.0 - (-seconds * rate).exp());
        if (wanted - now.strength).abs() < SETTLED {
            now.strength = wanted;
        }
        spot.set(now);
    });
    Hover(spot)
}
