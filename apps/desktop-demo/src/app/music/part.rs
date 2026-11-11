// One part of a score as a composable: it draws its notes falling toward a
// keyboard, and it sounds them through the performance around it. Leaving
// the composition silences the part.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{remember, DisposableEffect, SideEffect};
use cranpose_ui::{composable, Brush, Canvas, Color, CornerRadii, Modifier, Rect};

use super::{
    key_range::KeyRange,
    performance::local_performance,
    song::{note_seconds, Notes},
    voicing::local_voicing,
};

// Song seconds a roll shows above its lower edge.
const SECONDS_SHOWN: f32 = 4.0;
const NOTE_CORNER: f32 = 3.0;
// A note that sounds is drawn brighter than one still falling.
const FALLING_ALPHA: f32 = 0.72;
// The time the pedal lets a note ring past its key is drawn this faint.
const PEDAL_ALPHA: f32 = 0.22;

// Draws `notes` as a piano roll over `keys` and plays them. The sound takes
// the voicing around the call, so `Instrument`, `Transpose` and `Volume`
// change the part they wrap.
#[composable]
pub fn Part(notes: Notes, keys: KeyRange, color: Color, modifier: Modifier) {
    let Some(performance) = local_performance().current() else {
        return;
    };
    let voicing = local_voicing().current();
    let slot = remember(|| Rc::new(Cell::new(None))).with(Rc::clone);

    {
        let player = performance.player().clone();
        let slot = Rc::clone(&slot);
        let notes = notes.clone();
        DisposableEffect(notes.clone(), move |scope| {
            slot.set(player.add_part(&notes));
            scope.on_dispose(move || {
                if let Some(taken) = slot.take() {
                    player.remove_part(taken);
                }
            })
        });
    }
    {
        let player = performance.player().clone();
        SideEffect(move || {
            if let Some(taken) = slot.get() {
                player.voice_part(taken, voicing);
            }
        });
    }

    Canvas(modifier.clip_to_bounds(), move |scope| {
        let size = scope.size();
        let now = performance.seconds();
        let per_second = size.height / SECONDS_SHOWN;
        let first = notes.first_at_or_after(now - f64::from(notes.longest_ring()));
        for note in &notes.as_slice()[first..] {
            let until_start = note_seconds(f64::from(note.start) - now);
            if until_start > SECONDS_SHOWN {
                break;
            }
            if until_start + note.ring < 0.0 {
                continue;
            }
            let key = i16::from(note.key) + i16::from(voicing.transpose);
            let Some((left, width)) = u8::try_from(key)
                .ok()
                .and_then(|key| keys.note_span(key, size.width))
            else {
                continue;
            };
            let bottom = size.height - until_start * per_second;
            let mut bar = |seconds: f32, alpha: f32| {
                let height = (seconds * per_second).max(NOTE_CORNER);
                scope.draw_round_rect_at(
                    Rect {
                        x: left,
                        y: bottom - height,
                        width,
                        height,
                    },
                    Brush::solid(color.with_alpha(alpha)),
                    CornerRadii::uniform(NOTE_CORNER),
                );
            };
            if note.ring > note.held {
                bar(note.ring, PEDAL_ALPHA);
            }
            let sounding = until_start <= 0.0;
            bar(note.held, if sounding { 1.0 } else { FALLING_ALPHA });
        }
    });
}
