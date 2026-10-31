use std::{
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use cranpose_services::{
    MediaSourceHandle, MediaSourceOpener, clear_platform_media_source_opener, file_folder,
    folder_files, set_platform_media_source_opener, uri_for_path,
};

struct Opener {
    ui: thread::ThreadId,
    retained: AtomicUsize,
}
impl MediaSourceOpener for Opener {
    fn open(&self, _: &str) -> io::Result<MediaSourceHandle> {
        panic!("collecting playback references must not open audio")
    }
    fn reference(&self, path: &Path) -> io::Result<String> {
        assert_ne!(
            thread::current().id(),
            self.ui,
            "platform grants must not block the UI"
        );
        self.retained.fetch_add(1, Ordering::SeqCst);
        Ok(uri_for_path(path))
    }
}
struct Registration;
impl Drop for Registration {
    fn drop(&mut self) {
        clear_platform_media_source_opener();
    }
}

#[test]
fn a_folder_retains_one_grant_off_the_ui_and_reuses_it_for_nested_tracks() {
    let _serial = super::media_support::one_media_player_at_a_time();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("Album")).unwrap();
    let paths = [
        root.path().join("one.flac"),
        root.path().join("Album/02 # ž.opus"),
    ];
    for path in &paths {
        std::fs::write(path, b"original").unwrap();
    }
    let opener = Arc::new(Opener {
        ui: thread::current().id(),
        retained: AtomicUsize::new(0),
    });
    let registered: Arc<dyn MediaSourceOpener> = Arc::clone(&opener) as Arc<dyn MediaSourceOpener>;
    set_platform_media_source_opener(registered);
    let _registration = Registration;
    let stream = folder_files(file_folder(root.path()));
    let first = pollster::block_on(stream.next()).unwrap().unwrap();
    let first_uri = pollster::block_on(first.media_uri_async()).unwrap();
    assert_eq!(first_uri, uri_for_path(&paths[0]));
    assert_eq!(
        stream.produced(),
        Some(1),
        "first playback reference precedes discovery of the tree"
    );
    let second = pollster::block_on(stream.next()).unwrap().unwrap();
    assert_eq!(
        pollster::block_on(second.media_uri_async()).unwrap(),
        uri_for_path(&paths[1])
    );
    assert_eq!(
        pollster::block_on(first.media_uri_async()).unwrap(),
        first_uri
    );
    assert_eq!(opener.retained.load(Ordering::SeqCst), 1);
    for path in &paths {
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }
}
