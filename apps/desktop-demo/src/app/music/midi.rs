// Reads a standard MIDI file into a `Song`: its tempo changes become the
// seconds of each note, and its sustain pedal becomes the time a note rings.

use anyhow::{anyhow, bail, Result};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

use super::song::{note_seconds, Note, Notes, Song, SongPart};

const CHANNELS: usize = 16;
const KEYS: usize = 128;
const SUSTAIN_PEDAL: u8 = 64;
const PEDAL_DOWN_FROM: u8 = 64;
const MICROS_PER_BEAT_AT_120_BPM: u32 = 500_000;

struct TempoChange {
    tick: u64,
    seconds: f64,
    micros_per_beat: u32,
}

struct TempoMap {
    ticks_per_beat: f64,
    changes: Vec<TempoChange>,
}

impl TempoMap {
    fn new(ticks_per_beat: u16, mut tempos: Vec<(u64, u32)>) -> Self {
        tempos.sort_by_key(|(tick, _)| *tick);
        let ticks_per_beat = f64::from(ticks_per_beat);
        let mut changes = vec![TempoChange {
            tick: 0,
            seconds: 0.0,
            micros_per_beat: MICROS_PER_BEAT_AT_120_BPM,
        }];
        for (tick, micros_per_beat) in tempos {
            let last = &changes[changes.len() - 1];
            let seconds = last.seconds
                + (tick - last.tick) as f64 * f64::from(last.micros_per_beat)
                    / ticks_per_beat
                    / 1.0e6;
            changes.push(TempoChange {
                tick,
                seconds,
                micros_per_beat,
            });
        }
        Self {
            ticks_per_beat,
            changes,
        }
    }

    fn seconds_at(&self, tick: u64) -> f64 {
        let after = self.changes.partition_point(|change| change.tick <= tick);
        let change = &self.changes[after - 1];
        change.seconds
            + (tick - change.tick) as f64 * f64::from(change.micros_per_beat)
                / self.ticks_per_beat
                / 1.0e6
    }
}

struct TickNote {
    start: u64,
    end: u64,
    channel: u8,
    key: u8,
    velocity: u8,
}

struct TickTrack {
    name: Option<String>,
    notes: Vec<TickNote>,
}

// The spans of ticks a channel's sustain pedal is down for.
#[derive(Default)]
struct Pedal {
    spans: Vec<(u64, u64)>,
    down_since: Option<u64>,
}

impl Pedal {
    fn press(&mut self, tick: u64) {
        self.down_since.get_or_insert(tick);
    }

    fn move_to(&mut self, tick: u64, down: bool) {
        if down {
            self.press(tick);
        } else {
            self.lift(tick);
        }
    }

    fn lift(&mut self, tick: u64) {
        if let Some(down) = self.down_since.take() {
            self.spans.push((down, tick));
        }
    }

    // The tick the damper falls on a key released at `tick`.
    fn damped_at(&self, tick: u64) -> u64 {
        let after = self.spans.partition_point(|(down, _)| *down <= tick);
        match after.checked_sub(1).map(|index| self.spans[index]) {
            Some((_, up)) if up > tick => up,
            _ => tick,
        }
    }
}

// The key that is down on each key of one channel: its tick and velocity.
type KeysDown = [Option<(u64, u8)>; KEYS];

// What the tracks of a file say about the song as a whole.
#[derive(Default)]
struct Shared {
    tempos: Vec<(u64, u32)>,
    pedals: [Pedal; CHANNELS],
    last_tick: u64,
}

impl Shared {
    // Reads one track: its name and its notes, and what it adds to the
    // tempo and the pedals of the song.
    fn read_track(&mut self, events: &[TrackEvent<'_>]) -> TickTrack {
        let mut track = TickTrack {
            name: None,
            notes: Vec::new(),
        };
        let mut down: [KeysDown; CHANNELS] = [[None; KEYS]; CHANNELS];
        let mut tick = 0u64;
        for event in events {
            tick += u64::from(event.delta.as_int());
            match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(micros_per_beat)) => {
                    self.tempos.push((tick, micros_per_beat.as_int()));
                }
                TrackEventKind::Meta(MetaMessage::TrackName(name)) => {
                    track.name = Some(String::from_utf8_lossy(name).into_owned());
                }
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    let keys = &mut down[usize::from(channel)];
                    self.read_message(&mut track, keys, tick, channel, message);
                }
                _ => {}
            }
        }
        self.last_tick = self.last_tick.max(tick);
        track
    }

    fn read_message(
        &mut self,
        track: &mut TickTrack,
        keys: &mut KeysDown,
        tick: u64,
        channel: u8,
        message: MidiMessage,
    ) {
        let (key, velocity) = match message {
            MidiMessage::NoteOn { key, vel } => (key.as_int(), vel.as_int()),
            MidiMessage::NoteOff { key, .. } => (key.as_int(), 0),
            MidiMessage::Controller { controller, value }
                if controller.as_int() == SUSTAIN_PEDAL =>
            {
                self.pedals[usize::from(channel)].move_to(tick, value.as_int() >= PEDAL_DOWN_FROM);
                return;
            }
            _ => return,
        };
        if let Some((start, velocity)) = keys[usize::from(key)].take() {
            track.notes.push(TickNote {
                start,
                end: tick,
                channel,
                key,
                velocity,
            });
        }
        if velocity > 0 {
            keys[usize::from(key)] = Some((tick, velocity));
        }
    }

    // The notes of `track` in seconds, each ringing until its pedal lifts.
    fn notes(&self, track: &TickTrack, tempo: &TempoMap) -> Notes {
        Notes::new(
            track
                .notes
                .iter()
                .map(|note| {
                    let start = tempo.seconds_at(note.start);
                    let damped = self.pedals[usize::from(note.channel)].damped_at(note.end);
                    Note {
                        start: note_seconds(start),
                        held: note_seconds(tempo.seconds_at(note.end) - start),
                        ring: note_seconds(tempo.seconds_at(damped) - start),
                        key: note.key,
                        velocity: note.velocity,
                    }
                })
                .collect(),
        )
    }
}

pub fn read_midi(bytes: &[u8]) -> Result<Song> {
    let smf = Smf::parse(bytes).map_err(|error| anyhow!("not a MIDI file: {error}"))?;
    let Timing::Metrical(ticks_per_beat) = smf.header.timing else {
        bail!("the MIDI file counts time in video frames, which this reader does not convert");
    };

    let mut shared = Shared::default();
    let tracks: Vec<TickTrack> = smf
        .tracks
        .iter()
        .map(|events| shared.read_track(events))
        .collect();
    let last_tick = shared.last_tick;
    for pedal in &mut shared.pedals {
        pedal.lift(last_tick);
        pedal.spans.sort_by_key(|(down, _)| *down);
    }

    let tempo = TempoMap::new(ticks_per_beat.as_int(), std::mem::take(&mut shared.tempos));
    let parts: Vec<SongPart> = tracks
        .iter()
        .filter(|track| !track.notes.is_empty())
        .enumerate()
        .map(|(index, track)| SongPart {
            name: track
                .name
                .clone()
                .unwrap_or_else(|| format!("Part {}", index + 1)),
            notes: shared.notes(track, &tempo),
        })
        .collect();
    if parts.is_empty() {
        bail!("the MIDI file holds no notes");
    }
    Ok(Song { parts })
}
