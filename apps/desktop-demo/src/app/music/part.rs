// One part of a score as a composable: while it is in the composition, the
// performance around it sounds its notes and shows them in `color`. The
// sound takes the voicing around the call, so `Instrument`, `Transpose` and
// `Volume` change the part they wrap. Leaving the composition silences the
// part and takes its notes off the roll.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{remember, DisposableEffect, SideEffect};
use cranpose_ui::{composable, Color};

use super::{performance::local_performance, song::Notes, voicing::local_voicing};

#[composable]
pub fn Part(notes: Notes, color: Color) {
    let Some(performance) = local_performance().current() else {
        return;
    };
    let voicing = local_voicing().current();
    let slot = remember(|| Rc::new(Cell::new(None))).with(Rc::clone);

    {
        let performance = performance.clone();
        let slot = Rc::clone(&slot);
        DisposableEffect((notes.clone(), color), move |scope| {
            slot.set(performance.add_part(&notes, color));
            scope.on_dispose(move || {
                if let Some(taken) = slot.take() {
                    performance.remove_part(taken);
                }
            })
        });
    }
    SideEffect(move || {
        if let Some(taken) = slot.get() {
            performance.voice_part(taken, voicing);
        }
    });
}
