use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use cranpose_services::{
    MediaArtwork, MediaCommand, MediaMetadata, playback_progress, publish_media_command,
    uri_for_path,
};
use winit::window::Window;
use zbus::{
    blocking::{Connection, connection},
    interface,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

use super::{NowPlaying, Status};

// A client asks an MPRIS player for the position when it wants it.
pub(super) const POSITION_REFRESH: Option<Duration> = None;

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

// What the player interface answers from the bus's own thread.
#[derive(Default)]
struct Shown {
    status: Option<Status>,
    metadata: HashMap<String, OwnedValue>,
    track: Option<OwnedObjectPath>,
    seekable: bool,
}

pub(super) struct Controls {
    connection: Connection,
    shown: Arc<Mutex<Shown>>,
    tracks: u64,
    item: Option<MediaMetadata>,
    cover: Option<PathBuf>,
}

impl Controls {
    pub(super) fn new(window: &dyn Window) -> Option<Self> {
        let shown = Arc::<Mutex<Shown>>::default();
        let identity = window.title();
        let element = cranpose_services::application_id().unwrap_or_else(|| identity.clone());
        connect(&bus_name(&element), &identity, &shown)
            .map_err(|error| log::warn!("MPRIS is unavailable: {error}"))
            .ok()
            .map(|connection| Self {
                connection,
                shown,
                tracks: 0,
                item: None,
                cover: None,
            })
    }

    pub(super) fn show(&mut self, now: Option<&NowPlaying>) {
        let item = now.map(|now| &now.metadata);
        if self.item.as_ref() != item {
            self.item = item.cloned();
            self.tracks += 1;
            self.show_cover(now.and_then(|now| now.metadata.artwork.as_ref()));
        }
        let metadata = now.map_or_else(HashMap::new, |now| self.metadata_of(&now.metadata));
        {
            let mut shown = lock(&self.shown);
            shown.status = now.map(|now| now.status);
            shown.track = now.map(|_| track_path(self.tracks));
            shown.seekable = now.is_some_and(|now| now.metadata.duration.is_some());
            shown.metadata.clone_from(&metadata);
        }
        let playing = now.is_some();
        let changed: HashMap<&str, Value<'_>> = HashMap::from([
            (
                "PlaybackStatus",
                Value::from(status_name(now.map(|now| now.status))),
            ),
            ("Metadata", Value::from(metadata)),
            ("CanPlay", Value::from(playing)),
            ("CanPause", Value::from(playing)),
            (
                "CanSeek",
                Value::from(now.is_some_and(|now| now.metadata.duration.is_some())),
            ),
        ]);
        let body = (PLAYER, changed, Vec::<&str>::new());
        let sent = self.connection.emit_signal(
            None::<&str>,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &body,
        );
        if let Err(error) = sent {
            log::warn!("MPRIS could not announce the playing item: {error}");
        }
    }

    pub(super) fn show_position(&mut self, now: &NowPlaying) {
        let sent = self.connection.emit_signal(
            None::<&str>,
            PATH,
            PLAYER,
            "Seeked",
            &(micros(now.position),),
        );
        if let Err(error) = sent {
            log::warn!("MPRIS could not announce a seek: {error}");
        }
    }

    fn metadata_of(&self, metadata: &MediaMetadata) -> HashMap<String, OwnedValue> {
        let mut values: Vec<(&str, Value<'_>)> = vec![
            (
                "mpris:trackid",
                Value::from(ObjectPath::from(track_path(self.tracks))),
            ),
            ("xesam:title", Value::from(metadata.title.as_str())),
            ("xesam:album", Value::from(metadata.album.as_str())),
        ];
        if !metadata.artist.is_empty() {
            values.push(("xesam:artist", Value::from(vec![metadata.artist.as_str()])));
        }
        if let Some(duration) = metadata.duration {
            values.push(("mpris:length", Value::from(micros(duration))));
        }
        if let Some(cover) = &self.cover {
            values.push(("mpris:artUrl", Value::from(uri_for_path(cover))));
        }
        values
            .into_iter()
            .filter_map(|(key, value)| Some((key.to_owned(), OwnedValue::try_from(value).ok()?)))
            .collect()
    }

    // MPRIS takes the cover as a URL, so it is written to a file of its own.
    fn show_cover(&mut self, artwork: Option<&MediaArtwork>) {
        if let Some(old) = self.cover.take() {
            let _ = std::fs::remove_file(old);
        }
        self.cover = artwork.and_then(|artwork| {
            write_cover(artwork)
                .map_err(|error| log::warn!("MPRIS could not store the cover: {error}"))
                .ok()
        });
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        if let Some(cover) = self.cover.take() {
            let _ = std::fs::remove_file(cover);
        }
    }
}

fn connect(name: &str, identity: &str, shown: &Arc<Mutex<Shown>>) -> zbus::Result<Connection> {
    let build = |name: &str| {
        connection::Builder::session()?
            .name(name.to_owned())?
            .serve_at(
                PATH,
                Root {
                    identity: identity.to_owned(),
                },
            )?
            .serve_at(
                PATH,
                Player {
                    shown: Arc::clone(shown),
                },
            )?
            .build()
    };
    // A second instance of the app takes a name of its own, as MPRIS asks.
    build(name).or_else(|_| build(&format!("{name}.instance{}", std::process::id())))
}

// `org.mpris.MediaPlayer2.` and the app's name as one bus name element.
pub(super) fn bus_name(app: &str) -> String {
    let mut element: String = app
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if element.is_empty() || element.starts_with(|c: char| c.is_ascii_digit()) {
        element.insert(0, '_');
    }
    format!("org.mpris.MediaPlayer2.{element}")
}

fn write_cover(artwork: &MediaArtwork) -> std::io::Result<PathBuf> {
    let directory = cranpose_services::application_directories()
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .cache
        .join("media-artwork");
    std::fs::create_dir_all(&directory)?;
    let extension = match artwork.mime.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "img",
    };
    let mut hasher = DefaultHasher::new();
    artwork.bytes.hash(&mut hasher);
    let path = directory.join(format!("cover-{:016x}.{extension}", hasher.finish()));
    std::fs::write(&path, &artwork.bytes)?;
    Ok(path)
}

fn track_path(track: u64) -> OwnedObjectPath {
    ObjectPath::from_string_unchecked(format!("/org/cranpose/track/{track}")).into()
}

fn micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

pub(super) fn status_name(status: Option<Status>) -> &'static str {
    match status {
        Some(Status::Playing) => "Playing",
        Some(Status::Paused) => "Paused",
        Some(Status::Stopped) | None => "Stopped",
    }
}

fn lock(shown: &Mutex<Shown>) -> MutexGuard<'_, Shown> {
    shown.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Root {
    identity: String,
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

struct Player {
    shown: Arc<Mutex<Shown>>,
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

#[cfg(test)]
#[path = "../tests/mpris_tests.rs"]
mod tests;
