// The renderer of a performance. Its clock is a place in the song: the notes
// of the parts sound while the clock stands inside them. While the piece
// plays, the clock runs and each note starts on the frame its start falls
// on. A hand can also drag the clock, and then each note it enters sounds as
// far into its life as the clock stands past the note's start.

use std::sync::Arc;

use cranpose_audio::{
    backend::{Renderer, NOMINAL_CHANNELS, NOMINAL_SAMPLE_RATE},
    ring::{Consumer, Producer},
    RenderStatus,
};

use super::{
    reverb::Reverb,
    song::{note_seconds, Note, Notes},
    transport::Transport,
    voice::Voice,
    voice_bank::{Owner, Tag, VoiceBank},
    voicing::Voicing,
};

pub(super) const MAX_PARTS: u8 = 8;
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
    SetPart {
        slot: u8,
        notes: Notes,
        voicing: Voicing,
    },
    ClearPart {
        slot: u8,
    },
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

struct Part {
    notes: Notes,
    next: usize,
    voicing: Voicing,
}

// A clock under a hand: where the hand put it and how fast it goes there.
#[derive(Clone, Copy)]
struct Drag {
    target: f64,
    seconds_per_frame: f64,
}

pub(super) struct Synth {
    commands: Consumer<Command>,
    // Notes the synth lets go of return here, so the thread that made them
    // frees them.
    retired: Producer<Notes>,
    transport: Arc<Transport>,
    sample_rate: f32,
    channels: usize,
    position: f64,
    was_playing: bool,
    drag: Option<Drag>,
    parts: [Option<Part>; MAX_PARTS as usize],
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
    pub(super) fn new(
        commands: Consumer<Command>,
        retired: Producer<Notes>,
        transport: Arc<Transport>,
    ) -> Self {
        let mut reverb = Reverb::new();
        reverb.set_sample_rate(NOMINAL_SAMPLE_RATE);
        Self {
            commands,
            retired,
            transport,
            sample_rate: NOMINAL_SAMPLE_RATE,
            channels: NOMINAL_CHANNELS,
            position: 0.0,
            was_playing: false,
            drag: None,
            parts: [const { None }; MAX_PARTS as usize],
            bank: VoiceBank::new(),
            reverb,
        }
    }

    fn retire(&mut self, notes: Notes) {
        // The queue holds as many notes as there are commands in flight; a
        // full one means the other thread is gone, and its notes with it.
        if let Err(notes) = self.retired.push(notes) {
            std::mem::forget(notes);
        }
    }

    fn damp_parts(&mut self) {
        self.bank.release_where(|tag| tag.owner.is_part());
    }

    fn find_next_notes(&mut self) {
        for part in self.parts.iter_mut().flatten() {
            part.next = part.notes.first_at_or_after(self.position);
        }
    }

    fn apply_commands(&mut self, playing: bool) {
        while let Some(command) = self.commands.pop() {
            match command {
                Command::SetPart {
                    slot,
                    notes,
                    voicing,
                } => self.set_part(slot, notes, voicing),
                Command::ClearPart { slot } => {
                    if let Some(part) = self.parts[usize::from(slot)].take() {
                        self.retire(part.notes);
                    }
                    self.bank
                        .release_where(|tag| tag.owner == Owner::Part(slot));
                }
                Command::Seek(seconds) => {
                    self.position = seconds;
                    self.find_next_notes();
                    self.damp_parts();
                    if playing || self.drag.is_some() {
                        self.enter_notes(None);
                    }
                }
                Command::Drag(target) => self.drag_toward(target),
                Command::EndDrag => {
                    self.drag = None;
                    self.damp_parts();
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

    fn set_part(&mut self, slot: u8, notes: Notes, voicing: Voicing) {
        let place = &mut self.parts[usize::from(slot)];
        if let Some(part) = place {
            if part.notes.same_notes(&notes) {
                // The part keeps its own share of the notes, so this drop
                // frees nothing.
                part.voicing = voicing;
                return;
            }
        }
        let next = notes.first_at_or_after(self.position);
        let before = place.replace(Part {
            notes,
            next,
            voicing,
        });
        if let Some(part) = before {
            self.retire(part.notes);
        }
        self.bank
            .release_where(|tag| tag.owner == Owner::Part(slot));
    }

    fn drag_toward(&mut self, target: f64) {
        if (target - self.position).abs() > DRAG_JUMP_SECONDS {
            // Too far to pass through on the way: the clock is put there,
            // and the notes it lands in sound from where it landed.
            self.position = target;
            self.damp_parts();
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

    // Strikes the notes the clock stands in now and did not stand in at
    // `before`, each as far into its life as the clock is past its start,
    // and lets the dampers fall on the notes the clock left. `None` strikes
    // every note the clock stands in.
    fn enter_notes(&mut self, before: Option<f64>) {
        let now = self.position;
        for (slot, part) in (0u8..).zip(&self.parts) {
            let Some(part) = part else { continue };
            let earliest = now - f64::from(part.notes.longest_ring());
            let first = part.notes.first_at_or_after(earliest);
            let end = part.notes.started_by(now);
            for note in &part.notes.as_slice()[first..end] {
                let sounds = f64::from(note.start)..f64::from(note.silent_at());
                if !sounds.contains(&now) || before.is_some_and(|before| sounds.contains(&before)) {
                    continue;
                }
                let age = note_seconds(now - sounds.start);
                let key = f32::from(note.key) + f32::from(part.voicing.transpose);
                if !Voice::sounds_after(part.voicing.timbre.recipe(), key, age) {
                    continue;
                }
                self.bank.strike(
                    Tag {
                        owner: Owner::Part(slot),
                        key: note.key,
                        sounds_from: sounds.start,
                        silent_at: sounds.end,
                    },
                    note.velocity,
                    part.voicing,
                    age,
                    self.sample_rate,
                );
            }
        }
        self.bank.release_outside(now);
    }

    // Starts the notes that fall on the frame the running clock is at.
    fn strike_due(&mut self, step: f64) {
        let due_before = self.position + step * 0.5;
        for slot in 0..MAX_PARTS {
            while let Some((note, voicing)) = self.due_note(slot, due_before) {
                self.bank.strike(
                    Tag {
                        owner: Owner::Part(slot),
                        key: note.key,
                        // A note struck half a frame early sounds from now.
                        sounds_from: f64::from(note.start).min(self.position),
                        silent_at: f64::from(note.silent_at()),
                    },
                    note.velocity,
                    voicing,
                    0.0,
                    self.sample_rate,
                );
            }
        }
    }

    fn due_note(&mut self, slot: u8, due_before: f64) -> Option<(Note, Voicing)> {
        let part = self.parts[usize::from(slot)].as_mut()?;
        let note = *part.notes.as_slice().get(part.next)?;
        if f64::from(note.start) >= due_before {
            return None;
        }
        part.next += 1;
        Some((note, part.voicing))
    }

    // Frames from the clock to the start of the next note of any part.
    fn frames_to_next_note(&self, step: f64) -> usize {
        let next = self
            .parts
            .iter()
            .flatten()
            .filter_map(|part| part.notes.as_slice().get(part.next))
            .map(|note| f64::from(note.start))
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

    // Takes in whether the piece plays now: a clock that starts to run finds
    // its next notes, and one that stops lets the dampers fall, since it
    // would never reach the times they fall at.
    fn follow_transport(&mut self, playing: bool) {
        if playing && !self.was_playing {
            self.drag = None;
            self.find_next_notes();
        }
        if self.was_playing && !playing {
            self.damp_parts();
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
