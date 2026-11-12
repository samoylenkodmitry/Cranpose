// The handle of the thread that draws on a synth: it opens the output device
// at the first sound and tells the synth what the composition holds.

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
    held_notes::{Heard, HeldNotes, KeyDown, NotePlace, PartSlot},
    key_set::KeySet,
    song::{Note, Notes},
    synth::{Command, Synth},
    transport::Transport,
    voicing::Voicing,
};

// Room for everything a player holds, told to the synth at once when its
// device starts, and for the commands of the frames after.
const COMMAND_QUEUE: usize = 2048;

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

enum Device {
    // The synth waits for the first sound: a browser opens an output only
    // after the user acted on the page, and a tab nobody plays holds no device.
    Waiting(Box<Synth>),
    Open(Box<dyn AudioSink>),
    Resting(Box<dyn AudioSink>),
    Failed(String),
}

struct Inner {
    transport: Arc<Transport>,
    commands: RefCell<ring::Producer<Command>>,
    device: RefCell<Option<Device>>,
    output: MusicOutput,
    held: RefCell<HeldNotes>,
    dropped_commands: Cell<u32>,
}

// Plays the notes composables hold, and single keys, through a synth.
// Clones share the player.
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
        let transport = Arc::new(Transport::new());
        let synth = Synth::new(synth_commands, Arc::clone(&transport));
        Self {
            inner: Rc::new(Inner {
                transport,
                commands: RefCell::new(commands),
                device: RefCell::new(Some(Device::Waiting(Box::new(synth)))),
                output,
                held: RefCell::new(HeldNotes::new()),
                dropped_commands: Cell::new(0),
            }),
        }
    }

    fn is_listening(&self) -> bool {
        matches!(self.inner.device.borrow().as_ref(), Some(Device::Open(_)))
    }

    fn push(&self, command: Command) {
        if self.inner.commands.borrow_mut().push(command).is_err() {
            let dropped = self.inner.dropped_commands.get() + 1;
            self.inner.dropped_commands.set(dropped);
            log::warn!("music: the synth's queue is full; {dropped} commands dropped");
        }
    }

    // Tells the synth of a change. A synth whose device does not run takes
    // nothing from its queue, so it hears nothing here and all of it from
    // `tell_all` when the device starts.
    fn send(&self, command: Command) {
        if self.is_listening() {
            self.push(command);
        }
    }

    // Tells a synth whose device is about to start everything the player
    // holds, and where the clock stands.
    fn tell_all(&self) {
        self.push(Command::Seek(self.inner.transport.position()));
        let held = self.inner.held.borrow();
        for (slot, part) in held.parts() {
            self.push(Command::SetPart {
                slot: slot.0,
                voicing: part.voicing,
            });
        }
        self.push(Command::DropAll);
        for (place, note) in held.notes() {
            self.push(Command::HoldNote {
                place: place.0,
                part: note.part,
                note: note.note,
            });
        }
    }

    // Opens the device, or starts it again after a rest.
    fn wake(&self) {
        let asleep = matches!(
            self.inner.device.borrow().as_ref(),
            Some(Device::Waiting(_) | Device::Resting(_))
        );
        if !asleep {
            return;
        }
        // The synth hears all of it before its device starts: a device may
        // render inside the call that starts it, as Web Audio does, and a
        // synth that renders first writes its old clock over the player's.
        self.tell_all();
        let device = self.inner.device.borrow_mut().take();
        let woken = match device {
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
        *self.inner.device.borrow_mut() = woken;
    }

    // Stops the output device until the next sound. The clock stops with it.
    pub fn rest(&self) {
        let device = self.inner.device.borrow_mut().take();
        let resting = match device {
            Some(Device::Open(sink)) => {
                sink.suspend();
                Some(Device::Resting(sink))
            }
            other => other,
        };
        *self.inner.device.borrow_mut() = resting;
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
    // running; the held notes the clock stands in sound, each as far into
    // its life as the clock is past its start, and so does every held note
    // the clock enters on its way.
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

    // Holds the part that `notes` belong to, shown in `color`, until
    // `release_part`. A part held several times takes one slot. `None` when
    // every slot is taken.
    pub fn hold_part(&self, notes: &Notes, color: Color) -> Option<PartSlot> {
        let (slot, heard) = self.inner.held.borrow_mut().hold_part(notes.id(), color)?;
        if heard == Heard::Changed {
            self.send(Command::SetPart {
                slot: slot.0,
                voicing: Voicing::DEFAULT,
            });
        }
        Some(slot)
    }

    // Sets how the notes of the part in `slot` sound from their next strike on.
    pub fn voice_part(&self, slot: PartSlot, voicing: Voicing) {
        if self.inner.held.borrow_mut().voice_part(slot, voicing) == Heard::Changed {
            self.send(Command::SetPart {
                slot: slot.0,
                voicing,
            });
        }
    }

    // Lets go of one hold of the part in `slot`. The last one silences the
    // part and frees the slot.
    pub fn release_part(&self, slot: PartSlot) {
        if self.inner.held.borrow_mut().release_part(slot) == Heard::Changed {
            self.send(Command::ClearPart { slot: slot.0 });
        }
    }

    // Holds `note`, the one at `index` of the part in `slot`, until
    // `release_note`: the synth sounds it while its clock stands in it. A
    // note held several times takes one place. `None` when every place is
    // taken.
    pub fn hold_note(&self, slot: PartSlot, index: u32, note: Note) -> Option<NotePlace> {
        let (place, heard) = self.inner.held.borrow_mut().hold_note(slot, index, note)?;
        if heard == Heard::Changed {
            self.send(Command::HoldNote {
                place: place.0,
                part: slot.0,
                note,
            });
        }
        Some(place)
    }

    // Lets go of one hold of the note at `place`. With the last one the
    // note sounds no more.
    pub fn release_note(&self, place: NotePlace) {
        if self.inner.held.borrow_mut().release_note(place) == Heard::Changed {
            self.send(Command::DropNote { place: place.0 });
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

    // Calls `down` for each key the held notes keep down at `seconds` of
    // the song.
    pub fn for_each_key_down(&self, seconds: f64, down: impl FnMut(KeyDown)) {
        self.inner.held.borrow().for_each_key_down(seconds, down);
    }

    // The keys the held notes keep down at `seconds` of the song, each at
    // the pitch its part sounds it.
    pub fn keys_down_at(&self, seconds: f64) -> KeySet {
        let mut keys = KeySet::EMPTY;
        self.for_each_key_down(seconds, |down| keys.insert(down.key));
        keys
    }
}
