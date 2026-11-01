#![expect(
    unsafe_code,
    reason = "the WinRT interop that finds a window's media controls"
)]

use std::time::Duration;

use cranpose_services::{MediaArtwork, MediaCommand, publish_media_command};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Media::{
        MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
        SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    },
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference},
    Win32::{
        Foundation::HWND,
        System::WinRT::{
            ISystemMediaTransportControlsInterop, RO_INIT_MULTITHREADED, RoInitialize,
            RoUninitialize,
        },
    },
    core::{HSTRING, Ref, factory},
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

use super::{NowPlaying, Status};

// The overlay shows the position it was last given, so a playing item's
// position is sent again this often.
pub(super) const POSITION_REFRESH: Option<Duration> = Some(Duration::from_secs(5));

pub(super) struct Controls {
    controls: SystemMediaTransportControls,
    buttons: i64,
    seeks: i64,
}

impl Controls {
    pub(super) fn new(window: &dyn Window) -> Option<Self> {
        let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        Self::for_window(HWND(handle.hwnd.get() as *mut _))
            .map_err(|error| log::warn!("the system media controls are unavailable: {error}"))
            .ok()
    }

    fn for_window(window: HWND) -> windows::core::Result<Self> {
        let interop =
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        // The handle belongs to the primary window, which outlives these controls.
        let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(window) }?;
        controls.SetIsPlayEnabled(true)?;
        controls.SetIsPauseEnabled(true)?;
        controls.SetIsStopEnabled(true)?;
        controls.SetIsNextEnabled(true)?;
        controls.SetIsPreviousEnabled(true)?;
        let buttons = controls.ButtonPressed(&TypedEventHandler::new(
            |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
                if let Some(command) = args
                    .as_ref()
                    .and_then(|args| command_for(args.Button().ok()?))
                {
                    publish_media_command(command);
                }
                Ok(())
            },
        ))?;
        let seeks = controls.PlaybackPositionChangeRequested(&TypedEventHandler::new(
            |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    let position = Duration::from(args.RequestedPlaybackPosition()?);
                    publish_media_command(MediaCommand::SeekTo(position));
                }
                Ok(())
            },
        ))?;
        Ok(Self {
            controls,
            buttons,
            seeks,
        })
    }

    pub(super) fn show(&mut self, now: Option<&NowPlaying>) {
        if let Err(error) = self.try_show(now) {
            log::warn!("the system media controls refused the playing item: {error}");
        }
    }

    pub(super) fn show_position(&mut self, now: &NowPlaying) {
        if let Err(error) = self.show_timeline(now) {
            log::warn!("the system media controls refused the position: {error}");
        }
    }

    fn try_show(&self, now: Option<&NowPlaying>) -> windows::core::Result<()> {
        let updater = self.controls.DisplayUpdater()?;
        let Some(now) = now else {
            updater.ClearAll()?;
            self.controls
                .SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
            return self.controls.SetIsEnabled(false);
        };
        self.controls.SetIsEnabled(true)?;
        updater.SetType(MediaPlaybackType::Music)?;
        let music = updater.MusicProperties()?;
        music.SetTitle(&HSTRING::from(now.metadata.title.as_str()))?;
        music.SetArtist(&HSTRING::from(now.metadata.artist.as_str()))?;
        music.SetAlbumTitle(&HSTRING::from(now.metadata.album.as_str()))?;
        let thumbnail = now.metadata.artwork.as_ref().and_then(|artwork| {
            thumbnail_of(artwork)
                .map_err(|error| log::warn!("the media artwork could not be read: {error}"))
                .ok()
        });
        updater.SetThumbnail(thumbnail.as_ref())?;
        updater.Update()?;
        self.show_timeline(now)
    }

    fn show_timeline(&self, now: &NowPlaying) -> windows::core::Result<()> {
        self.controls.SetPlaybackStatus(match now.status {
            Status::Playing => MediaPlaybackStatus::Playing,
            Status::Paused => MediaPlaybackStatus::Paused,
            Status::Stopped => MediaPlaybackStatus::Stopped,
        })?;
        let end = now.metadata.duration.unwrap_or_default();
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(TimeSpan::default())?;
        timeline.SetMinSeekTime(TimeSpan::default())?;
        timeline.SetEndTime(TimeSpan::from(end))?;
        timeline.SetMaxSeekTime(TimeSpan::from(end))?;
        timeline.SetPosition(TimeSpan::from(now.position.min(end)))?;
        self.controls.UpdateTimelineProperties(&timeline)
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        let _ = self.controls.RemoveButtonPressed(self.buttons);
        let _ = self
            .controls
            .RemovePlaybackPositionChangeRequested(self.seeks);
        let _ = self
            .controls
            .DisplayUpdater()
            .and_then(|updater| updater.ClearAll());
        let _ = self.controls.SetIsEnabled(false);
    }
}

fn command_for(button: SystemMediaTransportControlsButton) -> Option<MediaCommand> {
    Some(match button {
        SystemMediaTransportControlsButton::Play => MediaCommand::Play,
        SystemMediaTransportControlsButton::Pause => MediaCommand::Pause,
        SystemMediaTransportControlsButton::Stop => MediaCommand::Stop,
        SystemMediaTransportControlsButton::Next => MediaCommand::Next,
        SystemMediaTransportControlsButton::Previous => MediaCommand::Previous,
        _ => return None,
    })
}

// Writes the cover into a stream on a thread of its own, which joins the
// multithreaded apartment for the write and leaves it after.
fn thumbnail_of(artwork: &MediaArtwork) -> windows::core::Result<RandomAccessStreamReference> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                unsafe { RoInitialize(RO_INIT_MULTITHREADED) }?;
                let stream = (|| {
                    let stream = InMemoryRandomAccessStream::new()?;
                    let writer = DataWriter::CreateDataWriter(&stream)?;
                    writer.WriteBytes(&artwork.bytes)?;
                    writer.StoreAsync()?.join()?;
                    writer.DetachStream()?;
                    stream.Seek(0)?;
                    RandomAccessStreamReference::CreateFromStream(&stream)
                })();
                unsafe { RoUninitialize() };
                stream
            })
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}
