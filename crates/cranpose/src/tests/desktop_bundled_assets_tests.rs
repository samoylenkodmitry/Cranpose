use super::*;

#[test]
fn a_path_that_climbs_out_of_the_root_is_refused() {
    assert!(safe_relative_path("../secrets").is_none());
    assert!(safe_relative_path("models/../../secrets").is_none());
    assert!(safe_relative_path("/etc/passwd").is_none());
    assert!(safe_relative_path("").is_none());
}

#[test]
fn an_ordinary_relative_path_survives_unchanged() {
    assert_eq!(
        safe_relative_path("models/detector.bin"),
        Some(PathBuf::from("models/detector.bin"))
    );
    assert_eq!(
        safe_relative_path("./models/detector.bin"),
        Some(PathBuf::from("models/detector.bin"))
    );
}

#[test]
fn an_asset_beside_the_executable_reads_and_streams_the_same_bytes() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let Some(directory) = executable.parent() else {
        return;
    };
    let name = "cranpose-desktop-asset-fixture.bin";
    let bytes: Vec<u8> = (0..cranpose_services::DEFAULT_CHUNK_LEN + 17)
        .map(|i| (i % 251) as u8)
        .collect();
    if std::fs::write(directory.join(name), &bytes).is_err() {
        return;
    }

    let assets = DesktopBundledAssets;
    let read = assets.read(name).expect("read");
    let mut streamed = Vec::new();
    let mut reader = assets.open(name).expect("open");
    let mut chunks = 0;
    while let Some(chunk) = reader.read_chunk().expect("chunk") {
        chunks += 1;
        streamed.extend_from_slice(&chunk);
    }
    let length = assets.len(name);
    let _ = std::fs::remove_file(directory.join(name));

    assert_eq!(read, bytes);
    assert_eq!(streamed, bytes);
    assert!(chunks > 1, "expected more than one chunk, got {chunks}");
    assert_eq!(length, Some(bytes.len() as u64));
}

#[test]
fn a_missing_asset_is_not_found_rather_than_a_read_failure() {
    let assets = DesktopBundledAssets;
    assert!(matches!(
        assets.read("cranpose-no-such-asset.bin"),
        Err(BundledAssetError::NotFound(_))
    ));
}
