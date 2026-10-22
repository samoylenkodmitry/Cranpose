#![expect(unsafe_code)]

//! The one audio session of an Apple app, which media playback and recording
//! share. It is active while either uses it, and released with a notice to
//! other apps once neither does, so their audio resumes and a Bluetooth
//! headset leaves its call mode. Neither user releases it under the other.

use std::sync::{Mutex, PoisonError};

use objc2_avf_audio::{AVAudioSession, AVAudioSessionSetActiveOptions};

/// What uses the session now.
struct Users {
    media: bool,
    recording: bool,
}

static USERS: Mutex<Users> = Mutex::new(Users {
    media: false,
    recording: false,
});

/// Marks media playback as using the session or done with it.
#[cfg(target_os = "ios")]
pub(crate) fn hold_for_media(held: bool) -> Result<(), String> {
    hold(|users| users.media = held)
}

/// Marks recording as using the session or done with it. The category that
/// takes input is set before the session is held.
pub(crate) fn hold_for_recording(held: bool) -> Result<(), String> {
    hold(|users| users.recording = held)
}

/// Applies `change` and activates or releases the session to match.
fn hold(change: impl FnOnce(&mut Users)) -> Result<(), String> {
    let mut users = USERS.lock().unwrap_or_else(PoisonError::into_inner);
    change(&mut users);
    // SAFETY: the shared session lives for the life of the process, and
    // activating or releasing it takes no other object.
    let applied = unsafe {
        let session = AVAudioSession::sharedInstance();
        if users.media || users.recording {
            session.setActive_error(true)
        } else {
            session.setActive_withOptions_error(
                false,
                AVAudioSessionSetActiveOptions::NotifyOthersOnDeactivation,
            )
        }
    };
    applied.map_err(|error| format!("audio session: {error}"))
}
