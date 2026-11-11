// The voices of a synth and what each of them plays: a note of a part, or a
// key under a hand.

use super::{
    voice::{Strike, Voice},
    voicing::Voicing,
};

const MAX_VOICES: usize = 48;
const HARDEST_STRIKE: f32 = 127.0;
const HIGHEST_KEY: i16 = 127;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Owner {
    // A note of the part in this slot.
    Part(u8),
    Hand,
}

impl Owner {
    pub(super) fn is_part(self) -> bool {
        matches!(self, Self::Part(_))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Tag {
    pub owner: Owner,
    pub key: u8,
    // Song seconds the note sounds from and the damper falls at.
    pub sounds_from: f64,
    pub silent_at: f64,
}

impl Tag {
    pub(super) fn hand(key: u8) -> Self {
        Self {
            owner: Owner::Hand,
            key,
            sounds_from: f64::NEG_INFINITY,
            silent_at: f64::INFINITY,
        }
    }

    fn sounds_at(&self, seconds: f64) -> bool {
        (self.sounds_from..self.silent_at).contains(&seconds)
    }
}

pub(super) struct VoiceBank {
    voices: [Voice; MAX_VOICES],
    tags: [Tag; MAX_VOICES],
}

impl VoiceBank {
    pub(super) fn new() -> Self {
        Self {
            voices: [Voice::IDLE; MAX_VOICES],
            tags: [Tag::hand(0); MAX_VOICES],
        }
    }

    // Starts a voice for `tag`, struck with a MIDI `velocity`, `age_seconds`
    // into the life of its note. With every voice taken the quietest one
    // gives way.
    pub(super) fn strike(
        &mut self,
        tag: Tag,
        velocity: u8,
        voicing: Voicing,
        age_seconds: f32,
        sample_rate: f32,
    ) {
        let index = self
            .voices
            .iter()
            .position(Voice::is_idle)
            .unwrap_or_else(|| self.quietest());
        let key = (i16::from(tag.key) + i16::from(voicing.transpose)).clamp(0, HIGHEST_KEY);
        self.voices[index].strike(
            voicing.timbre.recipe(),
            Strike {
                key: f32::from(key),
                velocity: f32::from(velocity) / HARDEST_STRIKE,
                gain: voicing.volume,
                age_seconds,
            },
            sample_rate,
        );
        self.tags[index] = tag;
    }

    fn quietest(&self) -> usize {
        let mut quietest = 0;
        for index in 1..MAX_VOICES {
            if self.voices[index].loudness() < self.voices[quietest].loudness() {
                quietest = index;
            }
        }
        quietest
    }

    pub(super) fn release_where(&mut self, released: impl Fn(&Tag) -> bool) {
        for (voice, tag) in self.voices.iter_mut().zip(&self.tags) {
            if released(tag) {
                voice.release();
            }
        }
    }

    // Lets the dampers fall on the notes of parts that do not sound at
    // `seconds` of the song.
    pub(super) fn release_outside(&mut self, seconds: f64) {
        for (voice, tag) in self.voices.iter_mut().zip(&self.tags) {
            if tag.owner.is_part() && voice.is_held() && !tag.sounds_at(seconds) {
                voice.release();
            }
        }
    }

    // Adds the next frames of every voice to `out`, left and right.
    pub(super) fn render(&mut self, out: &mut [[f32; 2]]) {
        for voice in &mut self.voices {
            voice.render(out);
        }
    }
}
