use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/cranpose-wfolder");
    let _ = std::fs::create_dir_all(&root);
    root.join(format!("{tag}-{nanos}-{n}"))
}

#[test]
fn round_trips_write_list_read_remove() {
    let dir = unique_dir("rw");
    let store = open(dir.to_string_lossy().as_ref());
    assert!(store.is_writable());

    store.write("a.bin", b"hello").expect("write");
    store.write("b.bin", b"world").expect("write");

    let mut names: Vec<String> = store
        .list()
        .expect("list")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["a.bin".to_string(), "b.bin".to_string()]);
    assert_eq!(store.entry("a.bin").expect("entry").len, 5);

    assert_eq!(store.read("a.bin").expect("read"), b"hello");
    assert_eq!(store.handle(), dir.to_string_lossy());

    store.remove("a.bin").expect("remove");
    let remaining: Vec<String> = store
        .list()
        .expect("list")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(remaining, vec!["b.bin".to_string()]);

    let leftover: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp") || n.contains("write-probe"))
        .collect();
    assert!(leftover.is_empty(), "stray files: {leftover:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn streams_chunks_in_and_out() {
    let dir = unique_dir("stream");
    let store = open(dir.to_string_lossy().as_ref());
    let payload = vec![9u8; DEFAULT_CHUNK_LEN + 7];

    let mut writer = store.open_write("big.bin").expect("open_write");
    writer.write_chunk(&payload).expect("write_chunk");
    writer.finish().expect("finish");

    let mut reader = store.open_read("big.bin").expect("open_read");
    let mut sizes = Vec::new();
    let mut round_trip = Vec::new();
    while let Some(chunk) = reader.read_chunk().expect("read_chunk") {
        sizes.push(chunk.len());
        round_trip.extend_from_slice(&chunk);
    }
    assert_eq!(sizes, vec![DEFAULT_CHUNK_LEN, 7]);
    assert_eq!(round_trip, payload);

    drop(store.open_write("abandoned.bin").expect("open_write"));
    assert!(!dir.join("abandoned.bin").exists());
    assert!(!dir.join("abandoned.bin.tmp").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_missing_is_not_found() {
    let dir = unique_dir("missing");
    let store = open(dir.to_string_lossy().as_ref());
    assert!(matches!(store.read("nope"), Err(FolderError::NotFound(_))));
    assert!(matches!(
        store.open_read("nope").err(),
        Some(FolderError::NotFound(_))
    ));
    assert!(store.list().expect("list").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
