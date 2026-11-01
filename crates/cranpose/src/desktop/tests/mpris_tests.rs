use cranpose_services::{
    HostController, PlatformDirectories, clear_application_id, clear_host_controller,
    observe_media_commands, set_application_id, set_host_controller,
};
use zbus::{
    blocking::{Proxy, fdo::PropertiesProxy},
    names::InterfaceName,
};

use super::*;

#[test]
fn an_app_name_becomes_one_bus_name_element() {
    assert_eq!(
        bus_name("io.cranamp.app"),
        "org.mpris.MediaPlayer2.io_cranamp_app"
    );
    assert_eq!(bus_name("7zip"), "org.mpris.MediaPlayer2._7zip");
    assert_eq!(bus_name(""), "org.mpris.MediaPlayer2._");
}

#[test]
fn a_stopped_or_closed_item_reads_as_stopped() {
    assert_eq!(status_name(Some(Status::Playing)), "Playing");
    assert_eq!(status_name(Some(Status::Paused)), "Paused");
    assert_eq!(status_name(None), "Stopped");
}

// Runs under `just mpris`, which provides a disposable session bus.
#[test]
#[ignore = "requires a session bus"]
fn a_client_reads_the_playing_item_and_its_commands_reach_the_app() {
    let scratch = crate::test_scratch_dir("mpris");
    set_host_controller(Arc::new(ScratchHost(scratch.clone())));
    set_application_id("cranpose-mpris-test").expect("application id");
    let shown = Arc::<Mutex<Shown>>::default();
    let name = bus_name("cranpose-mpris-test");
    let connection = connect(&name, "MPRIS test", &shown).expect("the session bus");
    let mut controls = Controls {
        connection,
        shown,
        tracks: 0,
        item: None,
        cover: None,
    };
    let mut metadata = MediaMetadata::titled("Bus Song");
    metadata.artist = "Artist".to_owned();
    metadata.duration = Some(Duration::from_secs(90));
    metadata.artwork = Some(MediaArtwork::new(vec![1, 2, 3], "image/png"));
    controls.show(Some(&NowPlaying {
        metadata,
        status: Status::Playing,
        position: Duration::ZERO,
    }));

    let client = Connection::session().expect("a client connection");
    let properties = (|| {
        PropertiesProxy::builder(&client)
            .destination(name.as_str())?
            .path(PATH)?
            .build()
    })()
    .expect("the player's properties");
    let player = InterfaceName::from_static_str(PLAYER).expect("interface name");
    let read = |property: &str| properties.get(player.clone(), property).expect(property);

    assert_eq!(String::try_from(read("PlaybackStatus")).unwrap(), "Playing");
    let metadata = HashMap::<String, OwnedValue>::try_from(read("Metadata")).unwrap();
    let text = |key: &str| String::try_from(metadata[key].try_clone().unwrap()).unwrap();
    assert_eq!(text("xesam:title"), "Bus Song");
    assert_eq!(
        i64::try_from(&metadata["mpris:length"]).unwrap(),
        90_000_000
    );
    let cover = text("mpris:artUrl");
    assert!(cover.starts_with("file://"), "{cover}");

    let heard = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&heard);
    let _observer = observe_media_commands(move |command| lock_commands(&sink).push(command));
    let methods = Proxy::new(&client, name.as_str(), PATH, PLAYER).expect("player proxy");
    methods.call_method("Next", &()).expect("Next");
    assert_eq!(*lock_commands(&heard), [MediaCommand::Next]);

    controls.show(None);
    assert_eq!(String::try_from(read("PlaybackStatus")).unwrap(), "Stopped");
    let path = cover.trim_start_matches("file://");
    assert!(
        !std::path::Path::new(path).exists(),
        "the cover file outlived the item"
    );
    clear_application_id();
    clear_host_controller();
    let _ = std::fs::remove_dir_all(scratch);
}

// The app's folders, all in the test's scratch folder.
struct ScratchHost(PathBuf);

impl HostController for ScratchHost {
    fn set_keep_screen_on(&self, _enabled: bool) {}
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        Some(PlatformDirectories {
            data: self.0.join("data"),
            config: self.0.join("config"),
            cache: self.0.join("cache"),
            documents: None,
            temporary: self.0.join("temporary"),
            shared: None,
        })
    }
    fn exit(&self) {}
    fn background(&self) {}
}

fn lock_commands(commands: &Mutex<Vec<MediaCommand>>) -> MutexGuard<'_, Vec<MediaCommand>> {
    commands.lock().unwrap_or_else(PoisonError::into_inner)
}
