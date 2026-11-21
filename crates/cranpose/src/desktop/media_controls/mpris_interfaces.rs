// `#[interface]` generates an `Interface` impl whose `get_all` returns std's
// `HashMap`, as the trait declares it. A lint level on the hand-written impl
// does not reach the generated one, so this module holds only the two
// interfaces and the level sits on the module.
#![expect(
    clippy::disallowed_types,
    reason = "the `Interface` impl that `#[interface]` generates names std's HashMap, as the trait's `get_all` returns it"
)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use cranpose_core::collections::map::HashMap;
use cranpose_services::{MediaCommand, playback_progress, publish_media_command};
use zbus::{
    interface,
    zvariant::{ObjectPath, OwnedValue},
};

use super::{Shown, lock, micros, status_name};

pub(super) struct Root {
    pub(super) identity: String,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}

    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> &'static [&'static str] {
        &[]
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> &'static [&'static str] {
        &[]
    }
}

pub(super) struct Player {
    pub(super) shown: Arc<Mutex<Shown>>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        publish_media_command(MediaCommand::Next);
    }

    fn previous(&self) {
        publish_media_command(MediaCommand::Previous);
    }

    fn pause(&self) {
        publish_media_command(MediaCommand::Pause);
    }

    fn play_pause(&self) {
        publish_media_command(MediaCommand::TogglePlayPause);
    }

    fn stop(&self) {
        publish_media_command(MediaCommand::Stop);
    }

    fn play(&self) {
        publish_media_command(MediaCommand::Play);
    }

    fn seek(&self, offset: i64) {
        let position = playback_progress().position;
        let distance = Duration::from_micros(offset.unsigned_abs());
        publish_media_command(MediaCommand::SeekTo(if offset < 0 {
            position.saturating_sub(distance)
        } else {
            position.saturating_add(distance)
        }));
    }

    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        let current = lock(&self.shown).track.clone();
        if position >= 0 && current.is_some_and(|current| current.as_ref() == track) {
            publish_media_command(MediaCommand::SeekTo(Duration::from_micros(
                position.unsigned_abs(),
            )));
        }
    }

    fn open_uri(&self, _uri: String) {}

    #[zbus(property)]
    fn playback_status(&self) -> String {
        status_name(lock(&self.shown).status).to_owned()
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        lock(&self.shown)
            .metadata
            .iter()
            .filter_map(|(key, value)| Some((key.clone(), value.try_clone().ok()?)))
            .collect()
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        micros(playback_progress().position)
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        lock(&self.shown).status.is_some()
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        lock(&self.shown).status.is_some()
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        lock(&self.shown).seekable
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}
