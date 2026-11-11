// The renderer of a performance: it starts each note of its parts on the
// frame the note falls on, and adds up the voices that sound.

use std::sync::Arc;

use cranpose_audio::{
    backend::{Renderer, NOMINAL_CHANNELS, NOMINAL_SAMPLE_RATE},
    ring::{Consumer, Producer},
    RenderStatus,
};

use super::{
    reverb::Reverb,
    song::{Note, Notes},
    transport::Transport,
    voice::Voice,
    voicing::Voicing,
};

pub(super) const MAX_PARTS: u8 = 8;
const MAX_VOICES: usize = 48;
// Keys go up at the ends of spans this long, and a key that goes up fades
// over many of them.
const SPAN_FRAMES: usize = 64;
const HARDEST_STRIKE: f32 = 127.0;
const HIGHEST_KEY: i16 = 127;
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
    Seek(f64),
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

#[derive(Clone, Copy, PartialEq)]
enum Owner {
    Part(u8),
    Hand,
}

#[derive(Clone, Copy)]
struct Tag {
    owner: Owner,
    key: u8,
    // Song seconds the damper falls at.
    silent_at: f64,
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
    parts: [Option<Part>; MAX_PARTS as usize],
    voices: [Voice; MAX_VOICES],
    tags: [Tag; MAX_VOICES],
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
            parts: [const { None }; MAX_PARTS as usize],
            voices: [Voice::IDLE; MAX_VOICES],
            tags: [Tag {
                owner: Owner::Hand,
                key: 0,
                silent_at: f64::INFINITY,
            }; MAX_VOICES],
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

    fn release_where(&mut self, released: impl Fn(&Tag) -> bool) {
        for (voice, tag) in self.voices.iter_mut().zip(&self.tags) {
            if released(tag) {
                voice.release();
            }
        }
    }

    fn apply_commands(&mut self) {
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
                    self.release_where(|tag| tag.owner == Owner::Part(slot));
                }
                Command::Seek(seconds) => {
                    self.position = seconds;
                    for part in self.parts.iter_mut().flatten() {
                        part.next = part.notes.first_at_or_after(seconds);
                    }
                    self.release_where(|tag| tag.owner != Owner::Hand);
                }
                Command::KeyDown {
                    key,
                    velocity,
                    voicing,
                } => self.strike(
                    Tag {
                        owner: Owner::Hand,
                        key,
                        silent_at: f64::INFINITY,
                    },
                    velocity,
                    voicing,
                ),
                Command::KeyUp { key } => {
                    self.release_where(|tag| tag.owner == Owner::Hand && tag.key == key);
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
        self.release_where(|tag| tag.owner == Owner::Part(slot));
    }

    fn strike(&mut self, tag: Tag, velocity: u8, voicing: Voicing) {
        let index = self
            .voices
            .iter()
            .position(Voice::is_idle)
            .unwrap_or_else(|| self.quietest_voice());
        let key = (i16::from(tag.key) + i16::from(voicing.transpose)).clamp(0, HIGHEST_KEY);
        self.voices[index].strike(
            voicing.timbre.recipe(),
            f32::from(key),
            f32::from(velocity) / HARDEST_STRIKE,
            voicing.volume,
            self.sample_rate,
        );
        self.tags[index] = tag;
    }

    fn quietest_voice(&self) -> usize {
        let mut quietest = 0;
        for index in 1..MAX_VOICES {
            if self.voices[index].loudness() < self.voices[quietest].loudness() {
                quietest = index;
            }
        }
        quietest
    }

    // Starts the notes that fall on the frame the clock is at.
    fn strike_due(&mut self, step: f64) {
        let due_before = self.position + step * 0.5;
        for slot in 0..MAX_PARTS {
            while let Some((note, voicing)) = self.due_note(slot, due_before) {
                self.strike(
                    Tag {
                        owner: Owner::Part(slot),
                        key: note.key,
                        silent_at: f64::from(note.silent_at()),
                    },
                    note.velocity,
                    voicing,
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

    fn release_due(&mut self) {
        let position = self.position;
        for (voice, tag) in self.voices.iter_mut().zip(&self.tags) {
            if voice.is_held() && tag.silent_at <= position {
                voice.release();
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
        self.apply_commands();
        let volume = self.transport.volume();
        let playing = self.transport.is_playing();
        if self.was_playing && !playing {
            // A stopped clock would never reach the time the dampers fall at.
            self.release_where(|tag| tag.owner != Owner::Hand);
        }
        self.was_playing = playing;
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
            }

            let mut block = [[0.0f32; 2]; SPAN_FRAMES];
            let block = &mut block[..span];
            for voice in &mut self.voices {
                voice.render(block);
            }
            self.reverb.render(block);

            let (now, later) = rest.split_at_mut(span * self.channels);
            for (frame, [left, right]) in now.chunks_exact_mut(self.channels).zip(block.iter()) {
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
            rest = later;

            if playing {
                self.position += span as f64 * step;
                self.release_due();
            }
        }

        if playing && self.position >= f64::from(self.transport.length()) {
            self.transport.set_playing(false);
        }
        self.transport.set_position(self.position);
        RenderStatus::Continue
    }
}
