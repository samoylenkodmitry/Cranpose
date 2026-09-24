use super::*;
use crate::content::{DEFAULT_CHUNK_LEN, collect_stream, folder_files, write_all};

fn temp_dir(name: &str) -> PathBuf {
    crate::test_scratch_dir(&format!("content-{name}"))
}

#[test]
fn a_file_streams_in_chunks_and_reports_its_length() {
    let root = temp_dir("chunks");
    let path = root.join("payload.bin");
    let payload = vec![3u8; DEFAULT_CHUNK_LEN + 5];
    std::fs::write(&path, &payload).unwrap();

    let content = file_content(&path);
    assert_eq!(content.metadata().name, "payload.bin");
    assert_eq!(content.metadata().len, Some(payload.len() as u64));

    let sizes = pollster::block_on(async {
        let reader = content.open().await.unwrap();
        let mut sizes = Vec::new();
        while let Some(chunk) = reader.read_chunk().await.unwrap() {
            sizes.push(chunk.len());
        }
        sizes
    });
    assert_eq!(sizes, vec![DEFAULT_CHUNK_LEN, 5]);
    assert_eq!(pollster::block_on(content.read_all()).unwrap(), payload);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_missing_file_reports_not_found() {
    let root = temp_dir("missing");
    let content = file_content(root.join("absent.bin"));
    assert!(matches!(
        pollster::block_on(content.read_all()),
        Err(ContentError::NotFound(_))
    ));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_folder_streams_its_whole_tree() {
    let root = temp_dir("tree");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("a.txt"), b"a").unwrap();
    std::fs::write(root.join("nested/b.txt"), b"b").unwrap();

    let stream = folder_files(file_folder(&root));
    let mut names: Vec<String> = pollster::block_on(collect_stream(&stream))
        .unwrap()
        .iter()
        .map(|file| file.metadata().name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["a.txt", "b.txt"]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_sink_commits_atomically_and_discards_unfinished_writes() {
    let root = temp_dir("sink");
    let destination = root.join("out/report.bin");

    let sink = FileSink::create(&destination).unwrap().handle();
    pollster::block_on(write_all(&sink, b"committed".to_vec())).unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"committed");

    let abandoned = root.join("out/abandoned.bin");
    {
        let sink = FileSink::create(&abandoned).unwrap();
        pollster::block_on(sink.write_chunk(b"partial".to_vec())).unwrap();
    }
    assert!(!abandoned.exists());
    assert!(!staging_path(&abandoned).exists());
    std::fs::remove_dir_all(&root).unwrap();
}
