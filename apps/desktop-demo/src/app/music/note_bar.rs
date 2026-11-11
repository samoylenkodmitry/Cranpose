// One note as a composable: a rectangle on the roll, placed over its key and
// at its time, that sounds while the line of the roll stands in it. It
// flashes as the line reaches it and fades when its key goes up. What is
// drawn is what plays: the composable holds its note in the performance
// while it is in the composition.
//
// A note that lasts past the end of its slice of the roll is composed again
// in the next one; the performance counts them as one note.

use cranpose_animation::{animateFloatAsState, spring, Lerp};
use cranpose_core::{derivedStateOf, DisposableEffect};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, Brush, Color, CornerRadii, DrawScope, Modifier, Rect,
};

use super::{
    part::local_part,
    performance::local_performance,
    roll::{local_roll_slice, RollSlice},
    song::Note,
    voicing::local_voicing,
};

const CORNER: f32 = 4.0;
// The time the pedal lets a note ring past its key is drawn this faint.
const PEDAL_ALPHA: f32 = 0.2;
// A note that waits for the line is drawn dimmer than one that sounds.
const WAITING_ALPHA: f32 = 0.78;
// How far a note that sounds is mixed toward white.
const SOUNDING_LIGHT: f32 = 0.35;
// The halo around a note that sounds, and the bright end every note leads with.
const HALO: f32 = 4.0;
const HALO_ALPHA: f32 = 0.3;
const HEAD_HEIGHT: f32 = 5.0;
const HEAD_LIGHT: f32 = 0.55;
// The spring a note lights and fades with. It swings past its light as the
// line reaches the note: that swing is the flash, which whitens the note
// and widens its halo.
const LIGHT_DAMPING: f32 = 0.45;
const LIGHT_STIFFNESS: f32 = 400.0;
const FLASH_GAIN: f32 = 4.0;
const FLASH_LIGHT: f32 = 0.6;

// Corners for a bar whose top end, lower end or both lie in this slice. A
// cut between two slices stays square.
fn corners(top_ends: bool, starts: bool, radius: f32) -> CornerRadii {
    let corner = |rounded: bool| if rounded { radius } else { 0.0 };
    CornerRadii {
        top_left: corner(top_ends),
        top_right: corner(top_ends),
        bottom_right: corner(starts),
        bottom_left: corner(starts),
    }
}

// What a note bar draws in one slice of the roll.
#[derive(Clone, Copy)]
struct NotePicture {
    color: Color,
    // Seconds of the slice the key of the note is down for, from the lower
    // end of the bar.
    held: f32,
    // Whether the note starts, its key lifts and its sound ends in this
    // slice. An end that lies in another slice is a square cut here.
    starts_here: bool,
    key_lifts_here: bool,
    pedal_ends_here: bool,
}

impl NotePicture {
    // Draws the bar at `light`: 0 while the note waits, 1 while it sounds,
    // more in the flash of its start.
    fn draw(self, scope: &mut dyn DrawScope, light: f32) {
        let Self {
            color,
            starts_here,
            key_lifts_here,
            ..
        } = self;
        let size = scope.size();
        scope.draw_round_rect(
            Brush::solid(color.with_alpha(PEDAL_ALPHA)),
            corners(self.pedal_ends_here, starts_here, CORNER),
        );
        if self.held <= 0.0 {
            return;
        }
        let height = RollSlice::height_of(self.held);
        let bar = Rect {
            x: 0.0,
            y: size.height - height,
            width: size.width,
            height,
        };
        let lit = light.clamp(0.0, 1.0);
        let flash = ((light - 1.0) * FLASH_GAIN).clamp(0.0, 1.0);
        if lit > 0.0 {
            // The halo goes past the ends of the note, and not past a cut
            // between two slices.
            let halo = HALO * (1.0 + flash);
            let reach = |ends_here: bool| if ends_here { halo } else { 0.0 };
            scope.draw_round_rect_at(
                Rect {
                    x: bar.x - halo,
                    y: bar.y - reach(key_lifts_here),
                    width: bar.width + halo * 2.0,
                    height: bar.height + reach(key_lifts_here) + reach(starts_here),
                },
                Brush::solid(color.with_alpha(HALO_ALPHA * lit)),
                corners(key_lifts_here, starts_here, CORNER + halo),
            );
        }
        let fill = color
            .with_alpha(WAITING_ALPHA)
            .lerp(&color.lerp(&Color::WHITE, SOUNDING_LIGHT), lit)
            .lerp(&Color::WHITE, flash * FLASH_LIGHT);
        scope.draw_round_rect_at(
            bar,
            Brush::solid(fill),
            corners(key_lifts_here, starts_here, CORNER),
        );
        if starts_here {
            // The end of the note that reaches the line first.
            scope.draw_round_rect_at(
                Rect {
                    y: size.height - HEAD_HEIGHT.min(height),
                    height: HEAD_HEIGHT.min(height),
                    ..bar
                },
                Brush::solid(color.lerp(&Color::WHITE, HEAD_LIGHT)),
                CornerRadii::uniform(CORNER),
            );
        }
    }
}

// Shows `note`, the one at `index` of the part around the call, in the
// slice of the roll around the call, and sounds it with the voicing around
// the call. Outside a roll or a part it does nothing.
#[composable]
pub fn NoteBar(note: Note, index: u32) {
    let (Some(slice), Some(part), Some(performance)) = (
        local_roll_slice().current(),
        local_part().current(),
        local_performance().current(),
    ) else {
        return;
    };
    let voicing = local_voicing().current();

    {
        let performance = performance.clone();
        let part = part.clone();
        DisposableEffect((part.clone(), index, note), move |scope| {
            let place = part
                .slot()
                .and_then(|slot| performance.hold_note(slot, index, note));
            scope.on_dispose(move || {
                if let Some(place) = place {
                    performance.release_note(place);
                }
            })
        });
    }

    let key = i16::from(note.key) + i16::from(voicing.transpose);
    let Some((left, width)) = u8::try_from(key)
        .ok()
        .and_then(|key| slice.keys.note_span(key, slice.width))
    else {
        return;
    };
    // Seconds within the slice, from its start: later is higher on the roll.
    let rings = (note.start - slice.from).max(0.0)..(note.silent_at() - slice.from).min(1.0);
    let held = rings.start..(note.key_released_at() - slice.from).min(1.0);
    let picture = NotePicture {
        color: part.color,
        // Seconds of the slice the key of the note is down for.
        held: (held.end - held.start).max(0.0),
        starts_here: note.start >= slice.from,
        key_lifts_here: note.key_released_at() <= slice.to(),
        pedal_ends_here: note.silent_at() <= slice.to(),
    };
    // The note redraws when it starts and when it stops to sound, and not
    // with each move of the clock.
    let sounds = derivedStateOf(move || part.sounds_now(&note, &performance));
    let light = animateFloatAsState(
        f32::from(u8::from(sounds.get())),
        spring(LIGHT_DAMPING, LIGHT_STIFFNESS),
        "NoteBar light",
    );

    CranposeBox(
        Modifier::empty()
            .offset(left, RollSlice::height_of(1.0 - rings.end))
            .size_points(width, RollSlice::height_of(rings.end - rings.start))
            .draw_behind(move |scope| picture.draw(scope, light.get())),
        BoxSpec::default(),
        || {},
    );
}
