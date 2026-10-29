use std::sync::{Arc, Mutex};

use cranpose_services::{
    BytesBody, ContentError, HttpMethod, HttpResponse, StubHttpClient, WebDavFolder, folder_files,
    http_body_ref,
};

#[test]
fn folder_yields_original_references_before_requesting_nested_directories() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&requests);
    let client = Arc::new(StubHttpClient::new(move |request| {
        observed
            .lock()
            .unwrap()
            .push((request.method, request.url.clone()));
        assert_eq!(request.method, HttpMethod::Propfind);
        assert!(request.headers.contains(&("Depth".into(), "1".into())));
        assert_eq!(request.url, "https://music.example/libr%61ry/");
        Ok(HttpResponse::new(
            request.url.clone(),
            207,
            http_body_ref(BytesBody::new(
                r#"<d:multistatus xmlns:d="DAV:">
            <d:response><d:href>/library/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
            <d:response><d:href>/library/later/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
            <d:response><d:href>/library/A%20%26%20B.flac</d:href><d:propstat><d:prop><d:getcontentlength>123456789</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
            </d:multistatus>"#,
            )),
        ))
    }));
    let folder = WebDavFolder::new("https://music.example/libr%61ry", client).unwrap();
    let stream = folder_files(folder.handle());
    let file = pollster::block_on(stream.next()).unwrap().unwrap();
    assert_eq!(file.metadata().name, "A & B.flac");
    assert_eq!(file.metadata().len, Some(123456789));
    assert_eq!(
        file.media_uri().unwrap(),
        "https://music.example/library/A%20%26%20B.flac"
    );
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "no audio or child listing may be fetched before the first entry"
    );
}

#[test]
fn failed_listing_is_an_error_instead_of_an_empty_folder() {
    let client = Arc::new(StubHttpClient::new(|request| {
        Ok(HttpResponse::empty(&request.url, 503))
    }));
    let stream = folder_files(
        WebDavFolder::new("https://music.example/library/", client)
            .unwrap()
            .handle(),
    );
    let error = pollster::block_on(stream.next())
        .err()
        .expect("listing must fail");
    assert!(matches!(error, ContentError::Io(_)));
}

fn listing(href: &str, properties: &str) -> String {
    format!(
        r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>{href}</d:href>{properties}</d:response></d:multistatus>"#
    )
}

fn propstat(properties: &str, status: u16) -> String {
    format!(
        "<d:propstat><d:prop>{properties}</d:prop><d:status>HTTP/1.1 {status}</d:status></d:propstat>"
    )
}

#[test]
fn split_properties_and_on_demand_reads_preserve_the_remote_item() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&requests);
    let properties = [
        propstat("<d:getcontentlength>4</d:getcontentlength>", 200),
        propstat("<d:getcontenttype>audio/flac</d:getcontenttype>", 200),
        propstat("<d:resourcetype><d:collection/></d:resourcetype>", 404),
    ]
    .concat();
    let xml = listing("song.flac", &properties);
    let client = Arc::new(StubHttpClient::new(move |request| {
        observed.lock().unwrap().push(request.method);
        let (status, bytes) = if request.method == HttpMethod::Propfind {
            (207, xml.as_bytes())
        } else {
            (200, b"fLaC".as_slice())
        };
        Ok(HttpResponse::new(
            &request.url,
            status,
            http_body_ref(BytesBody::new(bytes.to_vec())),
        ))
    }));
    let stream = folder_files(
        WebDavFolder::new("https://music.example/library/", client)
            .unwrap()
            .handle(),
    );
    let file = pollster::block_on(stream.next()).unwrap().unwrap();
    assert_eq!(file.metadata().mime_type.as_deref(), Some("audio/flac"));
    assert_eq!(file.metadata().len, Some(4));
    assert_eq!(*requests.lock().unwrap(), [HttpMethod::Propfind]);
    let reader = pollster::block_on(file.open()).unwrap();
    assert_eq!(
        pollster::block_on(reader.read_chunk()).unwrap().unwrap(),
        b"fLaC"
    );
    assert!(pollster::block_on(reader.read_chunk()).unwrap().is_none());
    assert_eq!(
        *requests.lock().unwrap(),
        [HttpMethod::Propfind, HttpMethod::Get]
    );
}

#[test]
fn malformed_or_out_of_scope_directory_entries_fail() {
    for href in [
        "https://other.example/song.flac",
        "/outside/song.flac",
        "sub/song.flac",
        "hidden%2Ffile.flac",
    ] {
        let xml = listing(href, &propstat("<d:resourcetype/>", 200));
        let client = Arc::new(StubHttpClient::new(move |r| {
            Ok(HttpResponse::new(
                &r.url,
                207,
                http_body_ref(BytesBody::new(xml.clone())),
            ))
        }));
        let stream = folder_files(
            WebDavFolder::new("https://music.example/library/", client)
                .unwrap()
                .handle(),
        );
        assert!(
            matches!(
                pollster::block_on(stream.next()),
                Err(ContentError::PermissionDenied(_))
            ),
            "{href}"
        );
    }
    for xml in ["not XML", "<html/>"] {
        let error = pollster::block_on(stream_for_xml(xml.to_owned()).next())
            .err()
            .unwrap();
        assert!(matches!(error, ContentError::Io(_)));
    }
}

fn stream_for_xml(xml: String) -> cranpose_services::ContentStreamRef {
    let client = Arc::new(StubHttpClient::new(move |r| {
        Ok(HttpResponse::new(
            &r.url,
            207,
            http_body_ref(BytesBody::new(xml.clone())),
        ))
    }));
    folder_files(
        WebDavFolder::new("https://music.example/library/", client)
            .expect("valid folder address")
            .handle(),
    )
}
