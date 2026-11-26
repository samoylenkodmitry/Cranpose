// What the microphone stream does before it reaches a device, through the public backend.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use cranpose_audio::backend::{self, InputFormat, InputPreset, Recorder};
use cranpose_services::{
    AudioError, MicrophoneAccess, MicrophonePermission, set_platform_microphone_access,
};

struct Counting {
    calls: Arc<AtomicUsize>,
}

impl Recorder for Counting {
    fn record(&mut self, _input: &[f32], _format: InputFormat, _captured: Option<Instant>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
}

// A platform whose person said no, which counts how often the stream readied its audio
struct Refused {
    prepared: Arc<AtomicUsize>,
}

impl MicrophoneAccess for Refused {
    fn permission(&self) -> MicrophonePermission {
        MicrophonePermission::Denied
    }

    fn request(&self) {}

    fn open_settings(&self) {}

    fn prepare_recording(&self) -> Result<(), String> {
        self.prepared.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

// One test, so that no other test of this binary sees the platform it installs.
#[test]
fn a_microphone_that_the_person_did_not_allow_opens_nothing() {
    let (calls, prepared) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    set_platform_microphone_access(Some(Arc::new(Refused {
        prepared: Arc::clone(&prepared),
    })));
    let opened = backend::open_microphone(
        Box::new(Counting {
            calls: Arc::clone(&calls),
        }),
        InputPreset::VoiceRecognition,
    );
    set_platform_microphone_access(None);
    assert_eq!(opened.err(), Some(AudioError::NotAllowed));
    assert_eq!(
        prepared.load(Ordering::Relaxed),
        0,
        "nothing readied the audio"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0, "the recorder got nothing");

    // Without a device backend, a microphone that the platform allows is not supported. A build
    // with a backend would open the real microphone here.
    if !cranpose_audio::has_device_backend() {
        let opened = backend::open_microphone(
            Box::new(Counting { calls }),
            InputPreset::VoiceCommunication,
        );
        assert_eq!(opened.err(), Some(AudioError::Unsupported));
    }
}

// A runner has no microphone, and a Mac asks the person before the first recording, so the test
// is for a machine with a microphone that may record:
// `cargo test -p cranpose-audio --features cpal-backend --test microphone -- --ignored`
#[test]
#[ignore = "needs a microphone and the permission to record"]
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for the device's own thread, and a poll is the way to see its calls"
)]
fn a_running_microphone_hands_its_samples_to_the_recorder() {
    let calls = Arc::new(AtomicUsize::new(0));
    let source = backend::open_microphone(
        Box::new(Counting {
            calls: Arc::clone(&calls),
        }),
        InputPreset::VoiceRecognition,
    )
    .expect("the default microphone opens");
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    while calls.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        calls.load(Ordering::Relaxed) > 0,
        "the recorder got samples"
    );
    assert!(source.is_running());
}
