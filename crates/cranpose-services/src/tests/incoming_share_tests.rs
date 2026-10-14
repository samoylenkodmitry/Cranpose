use super::*;

#[test]
fn bytes_become_readable_content_with_the_reported_name() {
    let item = IncomingContent::from_bytes(b"payload".to_vec())
        .with_name("note.txt")
        .with_mime_type("text/plain");
    let content = item.content().expect("bytes always resolve");
    assert_eq!(content.metadata().name, "note.txt");
    assert_eq!(content.metadata().mime_type.as_deref(), Some("text/plain"));
    assert_eq!(
        pollster::block_on(content.read_all()).expect("the bytes read back"),
        b"payload"
    );
}

#[test]
fn a_uri_item_names_itself_from_its_last_segment() {
    let item = IncomingContent::from_uri("content://media/external/images/42");
    assert_eq!(item.display_name(), "42");
}
