use cranpose_services::{BytesBody, HttpError, HttpResponse, StubHttpClient, http_body_ref};

use super::*;

fn payload(len: usize) -> Vec<u8> {
    (0..=255u8).cycle().take(len).collect()
}

fn range_of(request: &HttpRequest) -> u64 {
    request.resume_from.unwrap_or(0)
}

fn ranged_server(body: Vec<u8>) -> HttpClientRef {
    let requests = Arc::new(Mutex::new(Vec::new()));
    served(body, requests)
}

fn served(body: Vec<u8>, requests: Arc<Mutex<Vec<u64>>>) -> HttpClientRef {
    Arc::new(StubHttpClient::new(move |request| {
        let offset = range_of(request);
        requests.lock().push(offset);
        if offset as usize > body.len() {
            return Err(HttpError::HttpStatus {
                url: request.url.clone(),
                status: 416,
            });
        }
        let rest = body[offset as usize..].to_vec();
        let length = rest.len() as u64;
        Ok(
            HttpResponse::new(request.url.clone(), if offset > 0 { 206 } else { 200 }, {
                http_body_ref(BytesBody::chunked(rest, 64))
            })
            .with_headers(vec![("accept-ranges".to_owned(), "bytes".to_owned())])
            .with_content_length(Some(length))
            .with_resumed(offset > 0),
        )
    }))
}

fn unranged_server(body: Vec<u8>) -> HttpClientRef {
    Arc::new(StubHttpClient::new(move |request| {
        let length = body.len() as u64;
        Ok(HttpResponse::new(
            request.url.clone(),
            200,
            http_body_ref(BytesBody::chunked(body.clone(), 64)),
        )
        .with_content_length(Some(length)))
    }))
}

fn source(client: HttpClientRef) -> HttpSource {
    HttpSource::open_with("https://host/track.mp3", client)
        .expect("the stub answers")
        .0
}

#[test]
fn only_http_and_https_uris_are_claimed() {
    assert!(is_http_uri("https://host/track.mp3"));
    assert!(is_http_uri("HTTP://host/track.mp3"));
    assert!(!is_http_uri("file:///music/track.mp3"));
    assert!(!is_http_uri("content://media/audio/1"));
    assert!(!is_http_uri("/music/track.mp3"));
    assert!(!is_http_uri("https://"));
}

#[test]
fn the_whole_body_comes_back_in_order() {
    let body = payload(4096);
    let mut source = source(ranged_server(body.clone()));

    let mut read = Vec::new();
    source.read_to_end(&mut read).expect("read to end");

    assert_eq!(read, body);
}

#[test]
fn the_stated_length_is_reported_before_anything_is_read() {
    let source = source(ranged_server(payload(9001)));

    assert_eq!(source.byte_len(), Some(9001));
    assert!(source.is_seekable());
}

#[test]
fn a_seek_forward_asks_the_server_for_that_offset() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let body = payload(1 << 22);
    let mut source = source(served(body.clone(), Arc::clone(&requests)));

    source.seek(SeekFrom::Start(3_000_000)).expect("seek");
    let mut head = [0u8; 8];
    source.read_exact(&mut head).expect("read after the seek");

    assert_eq!(head, body[3_000_000..3_000_008]);
    assert_eq!(*requests.lock(), vec![0, 3_000_000]);
}

#[test]
fn a_seek_backwards_asks_the_server_again() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let body = payload(4096);
    let mut source = source(served(body.clone(), Arc::clone(&requests)));

    let mut head = [0u8; 2048];
    source.read_exact(&mut head).expect("read the head");
    source.seek(SeekFrom::Start(16)).expect("seek back");
    let mut again = [0u8; 8];
    source.read_exact(&mut again).expect("read the head again");

    assert_eq!(again, body[16..24]);
    assert_eq!(*requests.lock(), vec![0, 16]);
}

#[test]
fn a_short_hop_forward_stays_on_the_open_response() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let body = payload(8192);
    let mut source = source(served(body.clone(), Arc::clone(&requests)));

    source.seek(SeekFrom::Start(4096)).expect("seek");
    let mut tail = [0u8; 8];
    source.read_exact(&mut tail).expect("read after the hop");

    assert_eq!(tail, body[4096..4104]);
    assert_eq!(
        *requests.lock(),
        vec![0],
        "a hop inside the skip limit re-requested the body"
    );
}

#[test]
fn seeking_from_the_end_uses_the_stated_length() {
    let body = payload(2048);
    let mut source = source(ranged_server(body.clone()));

    assert_eq!(source.seek(SeekFrom::End(-4)).expect("seek"), 2044);
    let mut tail = [0u8; 4];
    source.read_exact(&mut tail).expect("read the tail");

    assert_eq!(tail, body[2044..]);
}

#[test]
fn reading_past_the_stated_end_reports_the_end_of_the_stream() {
    let mut source = source(ranged_server(payload(512)));

    source.seek(SeekFrom::Start(512)).expect("seek");

    assert_eq!(source.read(&mut [0u8; 16]).expect("read"), 0);
}

#[test]
fn seeking_before_the_start_is_an_error_rather_than_a_wrap() {
    let mut source = source(ranged_server(payload(64)));

    assert!(source.seek(SeekFrom::Current(-1)).is_err());
}

#[test]
fn a_server_without_ranges_plays_forward_and_refuses_to_seek() {
    let body = payload(4096);
    let mut source = source(unranged_server(body.clone()));

    assert!(!source.is_seekable());
    let mut head = [0u8; 2048];
    source.read_exact(&mut head).expect("read the head");
    assert_eq!(head.as_slice(), &body[..2048]);
    source.seek(SeekFrom::Start(16)).expect("seek is recorded");

    let error = source.read(&mut [0u8; 16]).expect_err("no ranges, no seek");
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn a_server_that_ignores_the_range_it_was_asked_for_is_refused() {
    let client: HttpClientRef = Arc::new(StubHttpClient::new(|request| {
        let body = payload(4096);
        Ok(HttpResponse::new(
            request.url.clone(),
            200,
            http_body_ref(BytesBody::new(body)),
        )
        .with_headers(vec![("accept-ranges".to_owned(), "bytes".to_owned())])
        .with_content_length(Some(4096)))
    }));
    let mut source = source(client);

    source
        .read_exact(&mut [0u8; 2048])
        .expect("read past the offset to come back to");
    source.seek(SeekFrom::Start(16)).expect("seek back");

    let error = source.read(&mut [0u8; 16]).expect_err("the range was lost");
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn a_refusing_server_fails_the_open_rather_than_the_first_read() {
    let client: HttpClientRef = Arc::new(StubHttpClient::new(|request| {
        Ok(HttpResponse::empty(request.url.clone(), 404))
    }));

    let error = HttpSource::open_with("https://host/gone.mp3", client)
        .map(drop)
        .expect_err("a 404 is not a stream");

    assert!(
        matches!(&error, MediaError::Failed(message) if message.contains("gone.mp3")),
        "{error:?}"
    );
}

#[test]
fn cancelling_ends_the_reads_that_follow() {
    let (mut source, cancel) =
        HttpSource::open_with("https://host/track.mp3", ranged_server(payload(4096)))
            .expect("the stub answers");

    cancel.cancel();

    assert_eq!(source.read(&mut [0u8; 16]).expect("cancelled"), 0);
}
