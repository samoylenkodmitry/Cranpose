#[test]
fn a_uri_that_cannot_be_decoded_exactly_is_refused_but_still_displays() {
    assert_eq!(
        percent_decode("Trip%20Photos").as_deref(),
        Some("Trip Photos")
    );
    assert_eq!(percent_decode_lossy("Trip%20Photos"), "Trip Photos");

    assert_eq!(percent_decode("bad%FFname"), None);
    assert_eq!(percent_decode_lossy("bad%FFname"), "bad\u{fffd}name");

    assert_eq!(percent_decode("cut%4"), None);
    assert_eq!(percent_decode_lossy("cut%4"), "cut%4");

    assert_eq!(percent_decode("100%zz"), None);
    assert_eq!(percent_decode_lossy("100%zz"), "100%zz");
}

use super::*;

fn block<T>(future: impl Future<Output = T>) -> T {
    pollster::block_on(future)
}

#[test]
fn metadata_reports_the_lowercase_extension() {
    let metadata = ContentMetadata::named("Scan.PNG");
    assert_eq!(metadata.extension().as_deref(), Some("png"));
    assert_eq!(ContentMetadata::named("noext").extension(), None);
    assert_eq!(ContentMetadata::named("trailing.").extension(), None);
}

#[test]
fn metadata_carries_what_a_provider_knew_about_the_item() {
    let metadata = ContentMetadata::named("Scan.png")
        .with_len(2_048)
        .with_modified_millis(1_700_000_000_000)
        .with_identifier("content://provider/17")
        .with_mime_type("image/png");

    assert_eq!(metadata.name, "Scan.png");
    assert_eq!(metadata.len, Some(2_048));
    assert_eq!(metadata.modified_millis, Some(1_700_000_000_000));
    assert_eq!(metadata.identifier, "content://provider/17");
    assert_eq!(metadata.mime_type.as_deref(), Some("image/png"));
    let bare = ContentMetadata::named("Scan.png");
    assert_eq!(bare.len, None);
    assert_eq!(bare.modified_millis, None);
    assert_eq!(bare.mime_type, None);
    assert_eq!(bare.identifier, "Scan.png");
}

#[test]
fn a_channel_reports_when_it_has_ended_and_ignores_what_arrives_after() {
    let channel = ContentChannel::new();
    assert!(!channel.is_closed());

    channel.close();
    assert!(channel.is_closed());

    channel.push(BytesContent::named("late.bin", vec![1]).handle());
    channel.fail(ContentError::Unsupported("after close"));
    assert!(channel.is_closed());

    let stream = channel.stream();
    assert!(
        block(stream.next())
            .expect("a closed channel ends cleanly")
            .is_none(),
        "a channel closed before anything was pushed must yield nothing"
    );
}

#[test]
fn bytes_content_streams_in_chunks_and_reads_whole() {
    let payload = vec![7u8; DEFAULT_CHUNK_LEN + 11];
    let content = BytesContent::named("blob.bin", payload.clone()).handle();
    assert_eq!(content.metadata().len, Some(payload.len() as u64));

    let chunks = block(async {
        let reader = content.open().await.unwrap();
        let mut sizes = Vec::new();
        while let Some(chunk) = reader.read_chunk().await.unwrap() {
            sizes.push(chunk.len());
        }
        sizes
    });
    assert_eq!(chunks, vec![DEFAULT_CHUNK_LEN, 11]);
    assert_eq!(block(content.read_all()).unwrap(), payload);
}

#[test]
fn walking_a_tree_yields_every_file_depth_first() {
    let nested = ReadyFolder::new(
        ContentMetadata::named("nested"),
        vec![ContentEntry::File(
            BytesContent::named("b.txt", b"b".to_vec()).handle(),
        )],
    )
    .handle();
    let root = ReadyFolder::new(
        ContentMetadata::named("root"),
        vec![
            ContentEntry::File(BytesContent::named("a.txt", b"a".to_vec()).handle()),
            ContentEntry::Folder(nested),
        ],
    )
    .handle();

    let stream = folder_files(root);
    let files = block(collect_stream(&stream)).unwrap();
    let names: Vec<String> = files.iter().map(|file| file.metadata().name).collect();
    assert_eq!(names, vec!["a.txt", "b.txt"]);
    assert_eq!(stream.produced(), Some(2));
}

#[test]
fn a_channel_wakes_its_collector_instead_of_being_polled() {
    let channel = Rc::new(ContentChannel::new());
    let stream = channel.stream();

    let mut future = Box::pin(stream.next());
    let waker = Waker::noop().clone();
    let mut context = Context::from_waker(&waker);
    assert!(future.as_mut().poll(&mut context).is_pending());

    channel.push(BytesContent::named("late.txt", b"late".to_vec()).handle());
    let Poll::Ready(Ok(Some(item))) = future.as_mut().poll(&mut context) else {
        panic!("the pushed item should have completed the pending collector");
    };
    assert_eq!(item.metadata().name, "late.txt");

    channel.close();
    assert!(matches!(block(stream.next()), Ok(None)));
    assert_eq!(stream.produced(), Some(1));
}

#[test]
fn a_failed_channel_reports_the_error_once_then_ends() {
    let channel = ContentChannel::new();
    let stream = channel.stream();
    channel.fail(ContentError::Io("provider died".into()));
    let Err(failure) = block(stream.next()) else {
        panic!("a failed channel reports its error to the collector");
    };
    assert_eq!(failure, ContentError::Io("provider died".into()));
    assert!(matches!(block(stream.next()), Ok(None)));
}
