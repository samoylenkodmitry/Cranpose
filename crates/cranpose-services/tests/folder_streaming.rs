use cranpose_services::{file_folder, folder_files};

#[test]
fn discovered_metadata_does_not_need_to_reopen_the_source() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("track.opus");
    std::fs::write(&path, b"audio stays at its source").unwrap();
    let stream = folder_files(file_folder(root.path()));
    let file = pollster::block_on(stream.next()).unwrap().unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        file.metadata().len,
        Some(b"audio stays at its source".len() as u64)
    );
    assert_eq!(file.metadata().name, "track.opus");
    assert!(pollster::block_on(stream.next()).unwrap().is_none());
}

#[test]
fn discovery_yields_a_root_file_before_opening_a_nested_directory() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("track.flac"), b"source").unwrap();
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let stream = folder_files(file_folder(root.path()));
    let first = pollster::block_on(stream.next()).unwrap().unwrap();
    assert_eq!(first.metadata().name, "track.flac");
    // The child has not been enumerated when the first track becomes available.
    std::fs::write(nested.join("later.wav"), b"later").unwrap();
    let second = pollster::block_on(stream.next()).unwrap().unwrap();
    assert_eq!(second.metadata().name, "later.wav");
    assert!(pollster::block_on(stream.next()).unwrap().is_none());
}

#[test]
#[ignore = "reads the folder supplied through CRANPOSE_FOLDER_PROBE"]
#[expect(
    clippy::disallowed_methods,
    reason = "explicit path for a manual read-only integration probe"
)]
fn mounted_folder_first_entry() {
    let path = std::env::var("CRANPOSE_FOLDER_PROBE").expect("folder path");
    let start = std::time::Instant::now();
    let stream = folder_files(file_folder(path));
    let file = pollster::block_on(stream.next())
        .unwrap()
        .expect("first file");
    println!(
        "First entry after {:?}; {} bytes reported; audio was not opened",
        start.elapsed(),
        file.metadata().len.unwrap_or_default()
    );
}
