// One note as a composable: a drop of rain that slides down the glass of
// the roll, over its key and at its time. Its round head leads: it reaches
// the line when the key goes down. Its body thins into a tail that is as
// long as the key stays down, and the time the pedal lets the note ring on
// is the faint trail above it. A shader draws the drop as the lens it is:
// it shows the window behind it turned over, and glows in the color of its
// part. The drop lights as the line of the roll reaches it and dims when
// its key goes up. What is drawn is what plays: the composable holds its
// note in the performance while it is in the composition.
//
// A note that lasts past the end of its slice of the roll is composed again
// in the next one; the performance counts them as one note. Each of them
// shows the part of the same drop that lies in its slice, so the drop has
// no seam.

use std::sync::{Arc, OnceLock};

use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::{derivedStateOf, DisposableEffect};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, Color, GraphicsLayer, Modifier, Point, Size,
};
use cranpose_ui_graphics::{RenderEffect, RuntimeShader, RUNTIME_SHADER_PRELUDE_WGSL};

use super::{
    part::local_part,
    performance::local_performance,
    roll::{local_roll_slice, RollSlice},
    song::Note,
    voicing::local_voicing,
};

// A drop is at least as long as its head, up to this many seconds of the
// roll: a short note is a bead.
pub(super) const LONGEST_BEAD_SECONDS: f32 = 0.25;
// The glow of a drop reaches this many radii of its head past the drop, up
// to this many seconds of the roll.
const GLOW_ROOM: f32 = 4.0;
pub(super) const MOST_GLOW_SECONDS: f32 = 0.5;
// The width of the head of a drop, in widths of its note on the roll.
const HEAD_WIDTH: f32 = 1.16;
// The spring a drop lights and dims with. It swings past its light as the
// line reaches the note: that swing is a flash of its glow.
const LIGHT_DAMPING: f32 = 0.45;
const LIGHT_STIFFNESS: f32 = 400.0;

fn drop_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(SOURCE.get_or_init(|| {
        Arc::from(format!(
            "{RUNTIME_SHADER_PRELUDE_WGSL}{}",
            include_str!("rain_drop.wgsl")
        ))
    }))
}

// The whole drop of a note, in the box of one of its pieces.
#[derive(Clone, Copy)]
struct DropPicture {
    color: Color,
    // The size of the box.
    size: Size,
    // The middle of the head.
    head: Point,
    radius: f32,
    // The heights of the tail over the middle of the head, and of the trail
    // over the tail.
    tail: f32,
    trail: f32,
    // How far the glow reaches past the drop.
    room: f32,
    // What tells this drop from another: no two streaks sway alike.
    seed: f32,
}

impl DropPicture {
    // The effect that draws the drop over what lies behind the box, at
    // `light`: 0 while the note waits, 1 while it sounds, more in the flash
    // of its start.
    fn effect(self, light: f32) -> RenderEffect {
        // The shader reads where on the window it draws: the water a drop
        // left keeps its shape on the glass.
        let mut shader = RuntimeShader::from_shared_source(drop_wgsl());
        // The shader reads what lies behind the box from a texture it shares
        // with the other drops, so all of them draw in one pass.
        shader.set_batched_source(true);
        shader.set_float2(0, self.size.width, self.size.height);
        shader.set_float2(2, self.head.x, self.head.y);
        shader.set_float(4, self.radius);
        shader.set_float(5, self.tail);
        shader.set_float(6, self.trail);
        shader.set_float(7, light);
        let Color(red, green, blue, alpha) = self.color;
        shader.set_float4(8, red, green, blue, alpha);
        shader.set_float(12, self.room);
        shader.set_float(13, self.seed);
        RenderEffect::runtime_shader(shader)
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
    // The head is a little wider than the key it falls on.
    let radius = width * HEAD_WIDTH * 0.5;
    // The times of the note's picture, in song seconds: the drop from the
    // press of the key, and the trail from the drop to the end of the sound.
    let bead = RollSlice::seconds_of(width).min(LONGEST_BEAD_SECONDS);
    let water = note.start..note.start + note.held.max(bead);
    let shown = note.start..water.end.max(note.silent_at());
    // The glow reaches past both ends of the note. Where it leaves this
    // slice, the piece in the next slice shows it: a box never leaves its
    // slice.
    let room = (radius * GLOW_ROOM).min(RollSlice::height_of(MOST_GLOW_SECONDS));
    let reach = RollSlice::seconds_of(room);
    let piece = (shown.start - reach).max(slice.from)..(shown.end + reach).min(slice.to());
    if piece.end <= piece.start {
        return;
    }
    // A later second lies higher on the roll: the height from one down to
    // an earlier one.
    let down = |later: f32, earlier: f32| RollSlice::height_of(later - earlier);
    let size = Size::new(width + room * 2.0, down(piece.end, piece.start));
    let picture = DropPicture {
        color: part.color,
        size,
        head: Point {
            x: room + width * 0.5,
            y: down(piece.end, water.start) - radius,
        },
        radius,
        tail: (down(water.end, water.start) - radius).max(0.0),
        trail: down(shown.end, water.end),
        room,
        seed: u16::try_from(index % 1024).map_or(0.0, f32::from),
    };
    // The note lights when it starts and dims when it stops to sound, and
    // does not follow each move of the clock.
    let sounds = derivedStateOf(move || part.sounds_now(&note, &performance));
    let light = animateFloatAsState(
        f32::from(u8::from(sounds.get())),
        spring(LIGHT_DAMPING, LIGHT_STIFFNESS),
        "NoteBar light",
    );

    CranposeBox(
        Modifier::empty()
            .offset(left - room, down(slice.to(), piece.end))
            .size_points(size.width, size.height)
            // The drop reads its light as it draws: a drop that lights
            // composes nothing again.
            .graphics_layer(move || GraphicsLayer {
                backdrop_effect: Some(picture.effect(light.get())),
                ..GraphicsLayer::default()
            }),
        BoxSpec::default(),
        || {},
    );
}
