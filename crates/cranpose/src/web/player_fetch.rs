//! Authenticated browser `fetch()`, in-memory `Blob` URLs, and manifest kind classification for
//! the Web [`PlayerBackend`](cranpose_services::PlayerBackend).

use cranpose_services::MediaItem;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, Headers, Request, RequestInit, RequestMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebManifestKind {
    Hls,
    Dash,
}

pub(crate) fn classify_manifest(item: &MediaItem) -> Option<WebManifestKind> {
    if let Some(mime) = item.mime_type.as_deref() {
        let lower = mime.to_ascii_lowercase();
        if lower.contains("mpegurl") {
            return Some(WebManifestKind::Hls);
        }
        if lower.contains("dash+xml") {
            return Some(WebManifestKind::Dash);
        }
    }
    let path = item.uri.split('?').next().unwrap_or(&item.uri);
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".m3u8") {
        Some(WebManifestKind::Hls)
    } else if lower.ends_with(".mpd") {
        Some(WebManifestKind::Dash)
    } else {
        None
    }
}

pub(crate) fn create_bytes_blob_url(bytes: &[u8], mime: &str) -> Result<String, String> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let bag = BlobPropertyBag::new();
    bag.set_type(mime);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &bag)
        .map_err(|err| format!("create Blob failed: {err:?}"))?;
    web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|err| format!("createObjectURL failed: {err:?}"))
}

fn build_request_headers(
    headers: &[(String, String)],
    byte_range: Option<(u64, u64)>,
) -> Result<Headers, String> {
    let js_headers = Headers::new().map_err(|err| format!("Headers::new: {err:?}"))?;
    for (name, value) in headers {
        js_headers
            .append(name, value)
            .map_err(|err| format!("header {name}: {err:?}"))?;
    }
    if let Some((length, offset)) = byte_range
        && length > 0
    {
        let end = offset.saturating_add(length).saturating_sub(1);
        js_headers
            .set("Range", &format!("bytes={offset}-{end}"))
            .map_err(|err| format!("Range header: {err:?}"))?;
    }
    Ok(js_headers)
}

pub(crate) async fn fetch_response(
    url: &str,
    headers: &[(String, String)],
    byte_range: Option<(u64, u64)>,
) -> Result<web_sys::Response, String> {
    let window = web_sys::window().ok_or_else(|| "browser window unavailable".to_owned())?;
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::Cors);
    if !headers.is_empty() || byte_range.is_some() {
        let js_headers = build_request_headers(headers, byte_range)?;
        init.set_headers(&js_headers);
    }
    let request = Request::new_with_str_and_init(url, &init)
        .map_err(|err| format!("Request::new: {err:?}"))?;
    let value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|err| format!("fetch {url}: {err:?}"))?;
    let response: web_sys::Response = value
        .dyn_into()
        .map_err(|_| "fetch did not return Response".to_owned())?;
    if !response.ok() {
        return Err(format!("HTTP {} for {url}", response.status()));
    }
    Ok(response)
}

pub(crate) async fn fetch_text(url: &str, headers: &[(String, String)]) -> Result<String, String> {
    let response = fetch_response(url, headers, None).await?;
    let promise = response
        .text()
        .map_err(|err| format!("Response::text: {err:?}"))?;
    let value = JsFuture::from(promise)
        .await
        .map_err(|err| format!("Response::text await: {err:?}"))?;
    value
        .as_string()
        .ok_or_else(|| "Response::text was not a string".to_owned())
}

pub(crate) async fn fetch_bytes(
    url: &str,
    headers: &[(String, String)],
    byte_range: Option<(u64, u64)>,
) -> Result<Vec<u8>, String> {
    let response = fetch_response(url, headers, byte_range).await?;
    let promise = response
        .array_buffer()
        .map_err(|err| format!("Response::array_buffer: {err:?}"))?;
    let buffer = JsFuture::from(promise)
        .await
        .map_err(|err| format!("Response::array_buffer await: {err:?}"))?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

pub(crate) async fn fetch_blob_url(
    url: &str,
    headers: &[(String, String)],
) -> Result<String, String> {
    let response = fetch_response(url, headers, None).await?;
    let promise = response
        .blob()
        .map_err(|err| format!("Response::blob: {err:?}"))?;
    let value = JsFuture::from(promise)
        .await
        .map_err(|err| format!("Response::blob await: {err:?}"))?;
    let blob: Blob = value
        .dyn_into()
        .map_err(|_| "Response::blob was not a Blob".to_owned())?;
    web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|err| format!("createObjectURL(blob): {err:?}"))
}
