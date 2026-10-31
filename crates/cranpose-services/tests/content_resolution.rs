use cranpose_services::{IncomingContent, resolve_content};

#[test]
fn incoming_files_reopen_names_with_uri_escapes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("10. Tensor #100% ž.mp3");
    std::fs::write(&path, b"original file").unwrap();
    let content = IncomingContent::from_file(&path).content().unwrap();
    assert_eq!(content.metadata().name, "10. Tensor #100% ž.mp3");
    assert_eq!(
        pollster::block_on(content.read_all()).unwrap(),
        b"original file"
    );
}

#[test]
fn opaque_references_need_a_platform_content_resolver() {
    assert!(resolve_content("apple-bookmark:1234/").is_none());
    assert!(resolve_content("content:provider/item").is_none());
}
