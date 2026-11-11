// The handle of the thread that draws on a synth: it opens the output device
// at the first sound and sends the synth its parts and keys.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use cranpose_audio::{
    backend::{self, AudioSink, Renderer},
    ring,
};
use cranpose_services::AudioError;
use cranpose_ui::Color;

use super::{
    key_set::KeySet,
    song::{note_seconds, Notes},
    synth::{Command, Synth, MAX_PARTS},
    transport::Transport,
    voicing::Voicing,
};

// A slider moves a part's voicing once a frame; the synth takes the queue
// empty about a hundred times a second.
const COMMAND_QUEUE: usize = 256;

type OpenSink = dyn Fn(Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError>;

// Where a player sends its sound. Clones share the output.
#[derive(Clone)]
pub struct MusicOutput(Rc<OpenSink>);

impl MusicOutput {
    // The output device of the platform.
    pub fn device() -> Self {
        thread_local! {
            static DEVICE: MusicOutput = MusicOutput::new(backend::open);
        }
        DEVICE.with(Self::clone)
    }

    // An output that hands the renderer of each player to `open_sink` at
    // the player's first sound.
    pub fn new(
        open_sink: impl Fn(Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError> + 'static,
    ) -> Self {
        Self(Rc::new(open_sink))
    }
}

impl PartialEq for MusicOutput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// One of the parts a player sounds at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartSlot(u8);

enum Device {
    // The synth waits for the first sound: a browser opens an output only
    // after the user acted on the page, and a tab nobody plays holds no device.
    Waiting(Box<Synth>),
    Open(Box<dyn AudioSink>),
    Resting(Box<dyn AudioSink>),
    Failed(String),
}

// A key a part holds down, as a picture shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyDown {
    // The key at the pitch its part sounds it.
    pub key: u8,
    // The color of its part.
    pub color: Color,
    // Seconds of the song since the key went down, and until it lifts.
    pub seconds_down: f32,
    pub seconds_left: f32,
}

// What a picture needs of a part.
#[derive(Clone, PartialEq)]
pub struct ShownPart {
    pub notes: Notes,
    // Semitones the part sounds above its written keys.
    pub transpose: i8,
    pub color: Color,
}

struct Inner {
    transport: Arc<Transport>,
    commands: RefCell<ring::Producer<Command>>,
    retired: RefCell<ring::Consumer<Notes>>,
    device: RefCell<Option<Device>>,
    output: MusicOutput,
    parts: RefCell<[Option<ShownPart>; MAX_PARTS as usize]>,
    dropped_commands: Cell<u32>,
}

// Plays parts and single keys through a synth. Clones share the player.
#[derive(Clone)]
pub struct MusicPlayer {
    inner: Rc<Inner>,
}

impl PartialEq for MusicPlayer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl MusicPlayer {
    // A player that opens `output` at its first sound.
    pub fn new(output: MusicOutput) -> Self {
        let (commands, synth_commands) = ring::channel(COMMAND_QUEUE);
        let (synth_retired, retired) = ring::channel(COMMAND_QUEUE);
        let transport = Arc::new(Transport::new());
        let synth = Synth::new(synth_commands, synth_retired, Arc::clone(&transport));
        Self {
            inner: Rc::new(Inner {
                transport,
                commands: RefCell::new(commands),
                retired: RefCell::new(retired),
                device: RefCell::new(Some(Device::Waiting(Box::new(synth)))),
                output,
                parts: RefCell::new([const { None }; MAX_PARTS as usize]),
                dropped_commands: Cell::new(0),
            }),
        }
    }

    fn send(&self, command: Command) {
        // Notes the synth let go of are freed here, off the audio thread.
        while self.inner.retired.borrow_mut().pop().is_some() {}
        if self.inner.commands.borrow_mut().push(command).is_err() {
            let dropped = self.inner.dropped_commands.get() + 1;
            self.inner.dropped_commands.set(dropped);
            log::warn!("music: the synth's queue is full; {dropped} commands dropped");
        }
    }

    // Opens the device, or starts it again after a rest.
    fn wake(&self) {
        let mut device = self.inner.device.borrow_mut();
        *device = match device.take() {
            Some(Device::Waiting(synth)) => match (self.inner.output.0)(synth) {
                Ok(sink) => Some(Device::Open(sink)),
                Err(error) => {
                    log::warn!("music: no output device: {error}");
                    Some(Device::Failed(error.to_string()))
                }
            },
            Some(Device::Resting(sink)) => {
                sink.resume();
                Some(Device::Open(sink))
            }
            other => other,
        };
    }

    // Stops the output device until the next sound. The clock stops with it.
    pub fn rest(&self) {
        let mut device = self.inner.device.borrow_mut();
        *device = match device.take() {
            Some(Device::Open(sink)) => {
                sink.suspend();
                Some(Device::Resting(sink))
            }
            other => other,
        };
    }

    // Why the player makes no sound, when the platform gave it no device.
    pub fn failure(&self) -> Option<String> {
        match self.inner.device.borrow().as_ref() {
            Some(Device::Failed(reason)) => Some(reason.clone()),
            _ => None,
        }
    }

    pub fn play(&self) {
        self.wake();
        self.inner.transport.set_playing(true);
    }

    pub fn pause(&self) {
        self.inner.transport.set_playing(false);
    }

    pub fn is_playing(&self) -> bool {
        self.inner.transport.is_playing()
    }

    // Moves the clock to `seconds` of the song.
    pub fn seek(&self, seconds: f64) {
        let seconds = seconds.max(0.0);
        self.inner.transport.set_position(seconds);
        self.send(Command::Seek(seconds));
    }

    // Drags the clock to `seconds` of the song by hand. The piece stops
    // running; the notes the clock stands in sound, each as far into its
    // life as the clock is past its start, and so does every note the clock
    // enters on its way.
    pub fn drag_to(&self, seconds: f64) {
        self.wake();
        self.inner.transport.set_playing(false);
        self.send(Command::Drag(seconds.max(0.0)));
    }

    // Lets go of a dragged clock: the dampers fall.
    pub fn end_drag(&self) {
        self.send(Command::EndDrag);
    }

    // Song seconds the synth has rendered up to.
    pub fn position(&self) -> f64 {
        self.inner.transport.position()
    }

    // Song seconds that pass in one second of sound.
    pub fn tempo(&self) -> f32 {
        self.inner.transport.tempo()
    }

    pub fn set_tempo(&self, tempo: f32) {
        self.inner.transport.set_tempo(tempo);
    }

    pub fn set_volume(&self, volume: f32) {
        self.inner.transport.set_volume(volume);
    }

    pub fn length(&self) -> f32 {
        self.inner.transport.length()
    }

    // Song seconds the clock stops at.
    pub fn set_length(&self, seconds: f32) {
        self.inner.transport.set_length(seconds);
    }

    // Takes a slot for `notes`. The part sounds once `voice_part` gives it
    // a voicing. `None` when every slot is taken.
    pub fn add_part(&self, notes: &Notes, color: Color) -> Option<PartSlot> {
        let mut parts = self.inner.parts.borrow_mut();
        let slot = (0..MAX_PARTS).find(|slot| parts[usize::from(*slot)].is_none())?;
        parts[usize::from(slot)] = Some(ShownPart {
            notes: notes.clone(),
            transpose: 0,
            color,
        });
        Some(PartSlot(slot))
    }

    // The parts the player holds, in the order of their slots.
    pub fn parts(&self) -> Vec<ShownPart> {
        self.inner
            .parts
            .borrow()
            .iter()
            .flatten()
            .cloned()
            .collect()
    }

    // Sounds the part of `slot` with `voicing` from its next note on.
    pub fn voice_part(&self, slot: PartSlot, voicing: Voicing) {
        let notes = {
            let mut parts = self.inner.parts.borrow_mut();
            let Some(part) = parts[usize::from(slot.0)].as_mut() else {
                return;
            };
            part.transpose = voicing.transpose;
            part.notes.clone()
        };
        self.send(Command::SetPart {
            slot: slot.0,
            notes,
            voicing,
        });
    }

    // Silences the part of `slot` and frees the slot.
    pub fn remove_part(&self, slot: PartSlot) {
        if self.inner.parts.borrow_mut()[usize::from(slot.0)]
            .take()
            .is_some()
        {
            self.send(Command::ClearPart { slot: slot.0 });
        }
    }

    // Strikes `key` by hand; it sounds until `key_up`.
    pub fn key_down(&self, key: u8, velocity: u8, voicing: Voicing) {
        self.wake();
        self.send(Command::KeyDown {
            key,
            velocity,
            voicing,
        });
    }

    pub fn key_up(&self, key: u8) {
        self.send(Command::KeyUp { key });
    }

    // Calls `held` for each key the parts hold down at `seconds` of the song.
    pub fn for_each_key_down(&self, seconds: f64, mut held: impl FnMut(KeyDown)) {
        for part in self.inner.parts.borrow().iter().flatten() {
            let earliest = seconds - f64::from(part.notes.longest_held());
            let started = part.notes.first_at_or_after(seconds);
            for note in part.notes.as_slice()[..started]
                .iter()
                .rev()
                .take_while(|note| f64::from(note.start) >= earliest)
            {
                let key = i16::from(note.key) + i16::from(part.transpose);
                if f64::from(note.key_released_at()) <= seconds {
                    continue;
                }
                if let Ok(key) = u8::try_from(key) {
                    held(KeyDown {
                        key,
                        color: part.color,
                        seconds_down: note_seconds(seconds - f64::from(note.start)),
                        seconds_left: note_seconds(f64::from(note.key_released_at()) - seconds),
                    });
                }
            }
        }
    }

    // The keys the parts hold down at `seconds` of the song, each at the
    // pitch its part sounds it.
    pub fn keys_down_at(&self, seconds: f64) -> KeySet {
        let mut down = KeySet::EMPTY;
        self.for_each_key_down(seconds, |held| down.insert(held.key));
        down
    }
}
