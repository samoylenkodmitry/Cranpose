// The lamp over the oil painting. It hangs up and to the left, low enough
// to rake across the paint, swings across as the page scrolls away and
// leans after the pointer. Its lean is animated, and the painting's light
// reads it where it is drawn, so a moving lamp composes nothing.

use cranpose_animation::{spring, Animatable, AnimationType, Spring};
use cranpose_core::{remember, with_current_composer};
use cranpose_ui::{composable, Modifier, PointerEventKind, PointerInputScope, Size};

// Where the lamp hangs: across and down in the stage's widths and heights,
// and how high above the paint in u.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LampPlace {
    pub across: f32,
    pub down: f32,
    pub height: f32,
}

// The lamp's lean after the pointer, from its resting place.
#[derive(Clone)]
pub(super) struct Lamp {
    across: Animatable<f32>,
    down: Animatable<f32>,
}

fn lean_animation() -> AnimationType {
    spring(Spring::DampingRatioNoBouncy, Spring::StiffnessVeryLow)
}

impl Lamp {
    // Where the lamp hangs now on a stage `stage_units` wide in u, with the
    // page scrolled `swing` of the way past it.
    pub(super) fn place(&self, swing: f32, stage_units: f32) -> LampPlace {
        let swing = swing.clamp(0.0, 1.0);
        let swing = swing * swing * (3.0 - 2.0 * swing);
        let wide = (stage_units / 1440.0 * 0.5 + 0.5).min(1.25);
        LampPlace {
            across: -0.40 + 0.56 * swing + self.across.state().get() * 0.85,
            down: -0.52 - 0.40 * swing + self.down.state().get() * 0.6,
            height: (445.0 - 140.0 * swing) * wide,
        }
    }

    fn lean_to(&self, across: f32, down: f32) {
        self.across.clone().animateTo(across, lean_animation());
        self.down.clone().animateTo(down, lean_animation());
    }

    // Makes the lamp lean after a pointer over `stage`, a node `size` big,
    // and back to rest when the pointer leaves.
    pub(super) fn follow_pointer(&self, stage: Modifier, size: Size) -> Modifier {
        let lamp = self.clone();
        stage.pointer_input(size.width.to_bits(), move |scope: PointerInputScope| {
            let lamp = lamp.clone();
            async move {
                scope
                    .await_pointer_event_scope(|events| async move {
                        loop {
                            let event = events.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Move | PointerEventKind::Enter => {
                                    let at = event.position;
                                    lamp.lean_to(
                                        (at.x / size.width.max(1.0)).clamp(0.0, 1.0) - 0.3,
                                        (at.y / size.height.max(1.0)).clamp(0.0, 1.0) - 0.35,
                                    );
                                }
                                PointerEventKind::Exit => lamp.lean_to(0.0, 0.0),
                                _ => {}
                            }
                        }
                    })
                    .await;
            }
        })
    }
}

// A lamp at rest, kept across compositions.
#[composable]
pub(super) fn rememberLamp() -> Lamp {
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    remember(move || Lamp {
        across: Animatable::new(0.0, runtime.clone()),
        down: Animatable::new(0.0, runtime),
    })
    .with(Clone::clone)
}
