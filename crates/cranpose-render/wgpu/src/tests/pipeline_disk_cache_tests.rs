use super::{PersistWatch, blob_key, current_blob, write_blob};

#[test]
fn a_burst_of_pipelines_persists_once_after_it_goes_quiet() {
    let mut watch = PersistWatch::default();
    assert!(!watch.observe(0));
    assert!(!watch.observe(3), "still growing");
    assert!(!watch.observe(5), "still growing");
    assert!(watch.observe(5), "quiet for a tick with new pipelines");
    assert!(!watch.observe(5), "written already");
}

#[test]
fn a_pipeline_reached_late_in_a_session_persists_too() {
    let mut watch = PersistWatch::default();
    watch.observe(19);
    assert!(watch.observe(19));
    for _ in 0..20 {
        assert!(!watch.observe(19));
    }
    assert!(!watch.observe(20));
    assert!(watch.observe(20));
}

fn scratch_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cranpose-pipeline-blob-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create blob directory");
    dir.join(name)
}

#[test]
fn a_written_blob_reads_back_as_the_drivers_data() {
    let path = scratch_file("written.bin");
    let data = [7u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    write_blob(&path, &data).expect("write blob");
    let file = std::fs::read(&path).expect("read blob");
    assert_eq!(current_blob(&file), Some(data.as_slice()));
    std::fs::remove_file(&path).expect("remove blob");
}

#[test]
fn a_blob_compiled_from_other_shaders_loads_cold() {
    let data = [9u8; 32];
    let mut other = blob_key();
    other[0] ^= 1;
    let foreign = [other.as_slice(), &data].concat();
    assert_eq!(current_blob(&foreign), None, "another build's key");
    assert_eq!(current_blob(&data), None, "a blob from before keys");
    assert_eq!(current_blob(&blob_key()[..4]), None, "a truncated key");
    let own = [blob_key().as_slice(), &data].concat();
    assert_eq!(current_blob(&own), Some(data.as_slice()));
}
