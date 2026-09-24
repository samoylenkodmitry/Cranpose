use super::*;

struct WholeAssets;

impl BundledAssets for WholeAssets {
    fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
        match path {
            "big.bin" => Ok(vec![4u8; crate::content::DEFAULT_CHUNK_LEN + 9]),
            "small.txt" => Ok(b"hello".to_vec()),
            other => Err(BundledAssetError::NotFound(other.to_string())),
        }
    }
}

#[test]
fn a_length_bounded_reader_stops_at_the_asset_and_not_at_the_stream() {
    let asset_len = crate::content::DEFAULT_CHUNK_LEN + 5;
    let mut package = vec![7u8; asset_len];
    package.extend_from_slice(&[9u8; 64]);

    let mut reader = StreamingAssetReader::with_length(
        "model.bin",
        std::io::Cursor::new(package),
        asset_len as u64,
    );
    let mut read = Vec::new();
    while let Some(chunk) = reader.read_chunk().expect("chunks read") {
        read.extend_from_slice(&chunk);
    }

    assert_eq!(
        read.len(),
        asset_len,
        "the asset ends where its length says"
    );
    assert!(
        read.iter().all(|byte| *byte == 7),
        "no byte of what follows the asset in the package is handed out"
    );
}

#[test]
fn the_default_reader_streams_a_whole_asset_in_chunks() {
    let assets = WholeAssets;
    let mut reader = assets.open("big.bin").expect("the asset opens");
    let mut sizes = Vec::new();
    let mut total = 0usize;
    while let Some(chunk) = reader.read_chunk().expect("chunks read") {
        sizes.push(chunk.len());
        total += chunk.len();
    }
    assert_eq!(sizes, vec![crate::content::DEFAULT_CHUNK_LEN, 9]);
    assert_eq!(total, crate::content::DEFAULT_CHUNK_LEN + 9);
}

#[test]
fn a_missing_asset_fails_to_open() {
    assert!(matches!(
        WholeAssets.open("absent.bin").err(),
        Some(BundledAssetError::NotFound(_))
    ));
}

#[test]
fn a_backend_that_cannot_stat_reports_no_length() {
    assert_eq!(WholeAssets.len("small.txt"), None);
}

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

#[test]
fn registration_round_trips() {
    let _guard = crate::registry::test_service_guard();
    struct Fake;
    impl BundledAssets for Fake {
        fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
            Ok(path.as_bytes().to_vec())
        }
    }
    set_platform_bundled_assets(Arc::new(Fake));
    assert_eq!(
        bundled_assets().unwrap().read("models/a").unwrap(),
        b"models/a"
    );
    clear_platform_bundled_assets();
    assert!(bundled_assets().is_none());
}

#[cfg(not(target_arch = "wasm32"))]
struct MapAssets(BTreeMap<String, Vec<u8>>);

#[cfg(not(target_arch = "wasm32"))]
impl BundledAssets for MapAssets {
    fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| BundledAssetError::NotFound(path.to_string()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn test_directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-output/bundled-assets")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn declarative_set_installs_and_detects_current_version() {
    let _guard = crate::registry::test_service_guard();
    let destination = test_directory();
    set_platform_bundled_assets(Arc::new(MapAssets(BTreeMap::from([
        ("models/a.bin".to_string(), vec![1, 2]),
        ("models/nested/b.bin".to_string(), vec![3]),
    ]))));
    let spec = BundledAssetInstallSpec::new("7", &destination)
        .source_root("models")
        .entry(BundledAssetEntry::new("a.bin"))
        .entry(BundledAssetEntry::mapped("nested/b.bin", "b.bin"));
    assert_eq!(
        install_bundled_asset_set(&spec).unwrap(),
        BundledAssetInstallOutcome::Installed
    );
    assert_eq!(std::fs::read(destination.join("a.bin")).unwrap(), [1, 2]);
    assert_eq!(std::fs::read(destination.join("b.bin")).unwrap(), [3]);
    assert_eq!(
        install_bundled_asset_set(&spec).unwrap(),
        BundledAssetInstallOutcome::Current
    );
    std::fs::remove_dir_all(destination).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn declaration_rejects_parent_paths_and_handles_missing_host() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_bundled_assets();
    let invalid = BundledAssetInstallSpec::new("1", test_directory())
        .entry(BundledAssetEntry::new("../outside"));
    assert!(matches!(
        install_bundled_asset_set(&invalid),
        Err(BundledAssetError::InvalidPath(_))
    ));
    let valid =
        BundledAssetInstallSpec::new("1", test_directory()).entry(BundledAssetEntry::new("inside"));
    assert_eq!(
        install_bundled_asset_set(&valid).unwrap(),
        BundledAssetInstallOutcome::Unavailable
    );
}
