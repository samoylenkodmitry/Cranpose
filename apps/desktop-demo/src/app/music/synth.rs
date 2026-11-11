// The renderer of a performance. It plays the notes that are held in it: a
// note composable holds its note here while it is in the composition. Its
// clock is a place in the song, and a held note sounds while the clock
// stands inside it. While the piece plays, the clock runs and each note
// starts on the frame its start falls on. A hand can also drag the clock,
// and then each note it enters sounds as far into its life as the clock
// stands past the note's start.

use std::sync::Arc;

use cranpose_audio::{
    backend::{Renderer, NOMINAL_CHANNELS, NOMINAL_SAMPLE_RATE},
    ring::Consumer,
    RenderStatus,
};

use super::{
    reverb::Reverb,
    song::{note_seconds, Note},
    transport::Transport,
    voice::Voice,
    voice_bank::{Owner, Tag, VoiceBank},
    voicing::Voicing,
};

pub(super) const MAX_PARTS: u8 = 8;
// The notes held at once: those of the seconds a roll composes around its
// line, in the densest passage of a piano piece.
pub(super) const MAX_NOTES: u16 = 512;
// Keys go up at the ends of spans this long, and a key that goes up fades
// over many of them.
const SPAN_FRAMES: usize = 64;
// A dragged clock reaches the place the hand put it within this time, so
// the steps of the hand, one a frame of the display, become one motion.
const DRAG_GLIDE_SECONDS: f64 = 0.02;
// A hand that moves the clock farther than this in one step, some fifteen
// times the speed of the piece, jumps: the clock does not sound the notes
// between.
const DRAG_JUMP_SECONDS: f64 = 0.25;
// The soft limit is exact below this input and flat above it.
const LIMIT_INPUT: f32 = 3.0;

pub(super) enum Command {
    // Sets how the notes of the part in `slot` sound.
    SetPart {
        slot: u8,
        voicing: Voicing,
    },
    // Lets the dampers fall on the notes of the part in `slot`.
    ClearPart {
        slot: u8,
    },
    // Holds `note` of the part in slot `part` at `place`.
    HoldNote {
        place: u16,
        part: u8,
        note: Note,
    },
    // Lets go of the note at `place`: it sounds no more.
    DropNote {
        place: u16,
    },
    // Lets go of every held note.
    DropAll,
    // Puts the clock at these song seconds.
    Seek(f64),
    // Drags the clock toward these song seconds while the piece does not play.
    Drag(f64),
    // Lets go of a dragged clock: the dampers fall.
    EndDrag,
    KeyDown {
        key: u8,
        velocity: u8,
        voicing: Voicing,
    },
    KeyUp {
        key: u8,
    },
}

// A note a composable holds.
#[derive(Clone, Copy)]
struct Held {
    part: u8,
    note: Note,
    // Whether the running clock has passed the start of the note.
    struck: bool,
}

impl Held {
    fn sounds(&self) -> std::ops::Range<f64> {
        f64::from(self.note.start)..f64::from(self.note.silent_at())
    }
}

// A clock under a hand: where the hand put it and how fast it goes there.
#[derive(Clone, Copy)]
struct Drag {
    target: f64,
    seconds_per_frame: f64,
}

pub(super) struct Synth {
    commands: Consumer<Command>,
    transport: Arc<Transport>,
    sample_rate: f32,
    channels: usize,
    position: f64,
    was_playing: bool,
    drag: Option<Drag>,
    parts: [Voicing; MAX_PARTS as usize],
    notes: [Option<Held>; MAX_NOTES as usize],
    bank: VoiceBank,
    reverb: Reverb,
}

// A limit with no corner: the sum of many voices bends toward full scale.
fn soft_limit(sample: f32) -> f32 {
    let sample = sample.clamp(-LIMIT_INPUT, LIMIT_INPUT);
    let square = sample * sample;
    sample * (27.0 + square) / (27.0 + 9.0 * square)
}

impl Synth {
    pub(super) fn new(commands: Consumer<Command>, transport: Arc<Transport>) -> Self {
        let mut reverb = Reverb::new();
        reverb.set_sample_rate(NOMINAL_SAMPLE_RATE);
        Self {
            commands,
            transport,
            sample_rate: NOMINAL_SAMPLE_RATE,
            channels: NOMINAL_CHANNELS,
            position: 0.0,
            was_playing: false,
            drag: None,
            parts: [Voicing::DEFAULT; MAX_PARTS as usize],
            notes: [None; MAX_NOTES as usize],
            bank: VoiceBank::new(),
            reverb,
        }
    }

    fn damp_notes(&mut self) {
        self.bank.release_where(|tag| tag.owner.is_note());
    }

    // Marks each held note by whether the clock has passed its start, so a
    // clock that runs from here strikes the notes ahead of it and no other.
    fn mark_struck(&mut self) {
        let position = self.position;
        for held in self.notes.iter_mut().flatten() {
            held.struck = f64::from(held.note.start) <= position;
        }
    }

    fn apply_commands(&mut self, playing: bool) {
        while let Some(command) = self.commands.pop() {
            match command {
                Command::SetPart { slot, voicing } => self.parts[usize::from(slot)] = voicing,
                Command::ClearPart { slot } => {
                    self.bank.release_where(|tag| tag.owner.is_of_part(slot));
                }
                Command::HoldNote { place, part, note } => {
                    self.hold(place, part, note, playing);
                }
                Command::DropNote { place } => {
                    self.notes[usize::from(place)] = None;
                    self.bank.release_where(|tag| tag.owner.is_at(place));
                }
                Command::DropAll => {
                    self.notes = [None; MAX_NOTES as usize];
                    self.damp_notes();
                }
                Command::Seek(seconds) => {
                    self.position = seconds;
                    self.damp_notes();
                    if playing || self.drag.is_some() {
                        self.enter_notes(None);
                    }
                    self.mark_struck();
                }
                Command::Drag(target) => self.drag_toward(target),
                Command::EndDrag => {
                    self.drag = None;
                    self.damp_notes();
                }
                Command::KeyDown {
                    key,
                    velocity,
                    voicing,
                } => self
                    .bank
                    .strike(Tag::hand(key), velocity, voicing, 0.0, self.sample_rate),
                Command::KeyUp { key } => {
                    self.bank
                        .release_where(|tag| tag.owner == Owner::Hand && tag.key == key);
                }
            }
        }
    }

    // Takes a note a composable holds from now on. A note the clock already
    // stands in, held late after a jump of the roll, sounds from where the
    // clock stands.
    fn hold(&mut self, place: u16, part: u8, note: Note, playing: bool) {
        let held = Held {
            part,
            note,
            struck: f64::from(note.start) <= self.position,
        };
        self.notes[usize::from(place)] = Some(held);
        if (playing || self.drag.is_some()) && held.sounds().contains(&self.position) {
            self.strike(place, held);
        }
    }

    // Strikes the note held at `place` as far into its life as the clock
    // stands past its start, unless it has faded by then.
    fn strike(&mut self, place: u16, held: Held) {
        let voicing = self.parts[usize::from(held.part)];
        let sounds = held.sounds();
        let age = note_seconds((self.position - sounds.start).max(0.0));
        let key = f32::from(held.note.key) + f32::from(voicing.transpose);
        if !Voice::sounds_after(voicing.timbre.recipe(), key, age) {
            return;
        }
        self.bank.strike(
            Tag {
                owner: Owner::Note {
                    part: held.part,
                    place,
                },
                key: held.note.key,
                // A note struck half a frame early sounds from now.
                sounds_from: sounds.start.min(self.position),
                silent_at: sounds.end,
            },
            held.note.velocity,
            voicing,
            age,
            self.sample_rate,
        );
    }

    fn drag_toward(&mut self, target: f64) {
        if (target - self.position).abs() > DRAG_JUMP_SECONDS {
            // Too far to pass through on the way: the clock is put there,
            // and the notes it lands in sound from where it landed.
            self.position = target;
            self.damp_notes();
            self.enter_notes(None);
        } else if self.drag.is_none() {
            // The hand takes the clock: the notes it stands in sound.
            self.enter_notes(None);
        }
        let frames = DRAG_GLIDE_SECONDS * f64::from(self.sample_rate);
        self.drag = Some(Drag {
            target,
            seconds_per_frame: (target - self.position) / frames,
        });
    }

    // Moves a dragged clock for `frames` and sounds the notes it enters.
    fn follow_drag(&mut self, drag: Drag, frames: usize) {
        let before = self.position;
        let step = drag.seconds_per_frame * frames as f64;
        let reached = if step >= 0.0 {
            (before + step).min(drag.target)
        } else {
            (before + step).max(drag.target)
        };
        if reached.to_bits() != before.to_bits() {
            self.position = reached;
            self.enter_notes(Some(before));
        }
    }

    // Strikes the held notes the clock stands in now and did not stand in at
    // `before`, and lets the dampers fall on the notes the clock left.
    // `None` strikes every held note the clock stands in.
    fn enter_notes(&mut self, before: Option<f64>) {
        let now = self.position;
        for place in 0..MAX_NOTES {
            let Some(held) = self.notes[usize::from(place)] else {
                continue;
            };
            let sounds = held.sounds();
            if sounds.contains(&now) && !before.is_some_and(|before| sounds.contains(&before)) {
                self.strike(place, held);
            }
        }
        self.bank.release_outside(now);
    }

    // Starts the held notes that fall on the frame the running clock is at.
    fn strike_due(&mut self, step: f64) {
        let due_before = self.position + step * 0.5;
        for place in 0..MAX_NOTES {
            let Some(held) = self.notes[usize::from(place)].as_mut() else {
                continue;
            };
            if held.struck || f64::from(held.note.start) >= due_before {
                continue;
            }
            held.struck = true;
            let held = *held;
            self.strike(place, held);
        }
    }

    // Frames from the clock to the start of the next held note.
    fn frames_to_next_note(&self, step: f64) -> usize {
        let next = self
            .notes
            .iter()
            .flatten()
            .filter(|held| !held.struck)
            .map(|held| f64::from(held.note.start))
            .fold(f64::INFINITY, f64::min);
        let frames = ((next - self.position) / step).ceil();
        if frames >= SPAN_FRAMES as f64 {
            return SPAN_FRAMES;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a whole count under SPAN_FRAMES; a count under one becomes one"
        )]
        let frames = frames as usize;
        frames.max(1)
    }

    // Takes in whether the piece plays now: a clock that starts to run
    // sounds the notes it stands in, unless a hand that dragged it there
    // already did, and one that stops lets the dampers fall, since it would
    // never reach the times they fall at.
    fn follow_transport(&mut self, playing: bool) {
        if playing && !self.was_playing {
            if self.drag.take().is_none() {
                self.enter_notes(None);
            }
            self.mark_struck();
        }
        if self.was_playing && !playing {
            self.damp_notes();
        }
        self.was_playing = playing;
    }

    // Writes the frames of `block` to `out`, which holds `self.channels`
    // samples a frame.
    fn write(&self, block: &[[f32; 2]], out: &mut [f32], volume: f32) {
        for (frame, [left, right]) in out.chunks_exact_mut(self.channels).zip(block) {
            let left = soft_limit(left * volume);
            let right = soft_limit(right * volume);
            match frame {
                [only] => *only = (left + right) * 0.5,
                [first, second, others @ ..] => {
                    *first = left;
                    *second = right;
                    others.fill(0.0);
                }
                [] => {}
            }
        }
    }
}

impl Renderer for Synth {
    fn set_device_format(&mut self, sample_rate: f32, channels: usize) {
        self.channels = channels.max(1);
        if sample_rate.to_bits() != self.sample_rate.to_bits() {
            self.sample_rate = sample_rate;
            self.reverb.set_sample_rate(sample_rate);
        }
    }

    fn render(&mut self, out: &mut [f32]) -> RenderStatus {
        let playing = self.transport.is_playing();
        self.follow_transport(playing);
        self.apply_commands(playing);
        let volume = self.transport.volume();
        let step = f64::from(self.transport.tempo()) / f64::from(self.sample_rate);

        let mut rest = out;
        while !rest.is_empty() {
            let mut span = (rest.len() / self.channels).min(SPAN_FRAMES);
            if span == 0 {
                rest.fill(0.0);
                break;
            }
            if playing {
                self.strike_due(step);
                span = span.min(self.frames_to_next_note(step));
            } else if let Some(drag) = self.drag {
                self.follow_drag(drag, span);
            }

            let mut block = [[0.0f32; 2]; SPAN_FRAMES];
            let block = &mut block[..span];
            self.bank.render(block);
            self.reverb.render(block);
            let (now, later) = rest.split_at_mut(span * self.channels);
            self.write(block, now, volume);
            rest = later;

            if playing {
                self.position += span as f64 * step;
                self.bank.release_outside(self.position);
            }
        }

        if playing && self.position >= f64::from(self.transport.length()) {
            self.transport.set_playing(false);
        }
        self.transport.set_position(self.position);
        RenderStatus::Continue
    }
}
