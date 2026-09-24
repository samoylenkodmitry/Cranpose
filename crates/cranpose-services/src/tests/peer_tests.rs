use super::*;

fn resolver_for(handle: &'static str, bytes: Vec<u8>) -> SourceResolver {
    Arc::new(move |requested: &str| {
        if requested == handle {
            Some(Arc::new(BytesSource::new(bytes.clone())) as Arc<dyn ByteSource>)
        } else {
            None
        }
    })
}

#[test]
fn round_trips_full_and_partial() {
    let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    let server = PeerServer::start("127.0.0.1:0", "secret", resolver_for("song", data.clone()))
        .expect("start");
    let base = format!("127.0.0.1:{}", server.port());

    let full = fetch_range(&base, "secret", "song", 0, None).expect("full");
    assert_eq!(full.bytes, data);
    assert_eq!(full.total_len, Some(5000));

    let part = fetch_range(&base, "secret", "song", 1000, Some(256)).expect("part");
    assert_eq!(part.bytes, data[1000..1256]);
    assert_eq!(part.total_len, Some(5000));

    assert_eq!(content_length(&base, "secret", "song").unwrap(), Some(5000));
}

#[test]
fn streams_to_writer_without_buffering() {
    let data: Vec<u8> = (0..2000u32).map(|i| i as u8).collect();
    let server =
        PeerServer::start("127.0.0.1:0", "k", resolver_for("s", data.clone())).expect("start");
    let base = format!("127.0.0.1:{}", server.port());
    let mut out = Vec::new();
    let total = fetch_to_writer(&base, "k", "s", 0, None, &mut out).expect("stream");
    assert_eq!(out, data);
    assert_eq!(total, Some(2000));
}

#[test]
fn rejects_wrong_token() {
    let server =
        PeerServer::start("127.0.0.1:0", "right", resolver_for("a", vec![1, 2, 3])).expect("s");
    let base = format!("127.0.0.1:{}", server.port());
    assert!(matches!(
        fetch_range(&base, "wrong", "a", 0, None),
        Err(PeerError::Unauthorized)
    ));
}

#[test]
fn unknown_handle_is_not_found() {
    let server =
        PeerServer::start("127.0.0.1:0", "t", resolver_for("a", vec![1, 2, 3])).expect("s");
    let base = format!("127.0.0.1:{}", server.port());
    assert!(matches!(
        fetch_range(&base, "t", "missing", 0, None),
        Err(PeerError::NotFound)
    ));
}

#[test]
fn handle_is_percent_encoded_round_trip() {
    let server =
        PeerServer::start("127.0.0.1:0", "t", resolver_for("a b/c.mp3", vec![9, 8, 7])).expect("s");
    let base = format!("127.0.0.1:{}", server.port());
    let got = fetch_range(&base, "t", "a b/c.mp3", 0, None).expect("fetch");
    assert_eq!(got.bytes, vec![9, 8, 7]);
}
