use anyhow::{anyhow, Context};
use cranpose_services::{local_http_client, local_uri_handler, HttpClientRef, HttpError};
use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, ButtonSpec, Color, Column, ColumnSpec, ContentScale,
    Image, ImageBitmap, LinearArrangement, Modifier, Row, RowSpec, ScrollState, Size, Spacer, Text,
    TextStyle, VerticalAlignment,
};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct XkcdResponse {
    num: u32,
    title: String,
    alt: String,
    img: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LoadedComic {
    comic: XkcdResponse,
    bitmap: ImageBitmap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum XkcdState {
    Loading,
    Ready(LoadedComic),
    Error(String),
}

fn random_u32(max_exclusive: u32) -> u32 {
    if max_exclusive <= 1 {
        return 1;
    }
    let raw = super::demo_random_u32();
    (raw % max_exclusive).max(1)
}

fn decode_bitmap(bytes: &[u8]) -> anyhow::Result<ImageBitmap> {
    let image = image::load_from_memory(bytes).context("failed to decode image bytes")?;
    let rgba = image.to_rgba8();
    ImageBitmap::from_rgba8(image.width(), image.height(), rgba.into_raw())
        .map_err(|err| anyhow!("invalid RGBA bitmap: {err}"))
}

fn cors_url(url: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        format!(
            "https://cranpose-cors-proxy.cranpose.workers.dev/?url={}",
            url_encode(url)
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        url.to_string()
    }
}

#[cfg(target_arch = "wasm32")]
fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    encoded
}

async fn fetch_latest_comic_num(client: &HttpClientRef) -> Result<u32, String> {
    let body = client
        .get_text(&cors_url("https://xkcd.com/info.0.json"))
        .await
        .map_err(|err| format!("failed to fetch latest xkcd info: {err}"))?;
    let latest: XkcdResponse = serde_json::from_str(&body)
        .map_err(|err| format!("failed to parse latest xkcd JSON: {err}"))?;
    Ok(latest.num.max(1))
}

async fn fetch_random_comic(client: &HttpClientRef) -> Result<LoadedComic, String> {
    let latest_num = fetch_latest_comic_num(client).await?;

    for _ in 0..8 {
        let candidate = random_u32(latest_num.saturating_add(1)).clamp(1, latest_num);
        let info_url = cors_url(&format!("https://xkcd.com/{candidate}/info.0.json"));

        let info_body = match client.get_text(&info_url).await {
            Ok(body) => body,
            Err(HttpError::HttpStatus { status: 404, .. }) => continue,
            Err(err) => return Err(format!("failed to fetch comic metadata: {err}")),
        };

        let comic: XkcdResponse = serde_json::from_str(&info_body)
            .map_err(|err| format!("failed to parse comic metadata: {err}"))?;

        let image_bytes = client
            .get_bytes(&cors_url(&comic.img))
            .await
            .map_err(|err| format!("failed to download comic image: {err}"))?;

        let bitmap = decode_bitmap(&image_bytes)
            .map_err(|err| format!("failed to decode comic image: {err}"))?;
        return Ok(LoadedComic { comic, bitmap });
    }

    Err("failed to fetch a valid random xkcd comic after multiple attempts".to_string())
}

#[composable]
pub(crate) fn xkcd_tab() {
    let state = cranpose_core::rememberMutableStateOf(|| XkcdState::Loading);
    let request_id = cranpose_core::rememberMutableStateOf(|| 1u64);
    let http_client = local_http_client().current();
    let uri_handler = local_uri_handler().current();

    cranpose_core::LaunchedEffect!(request_id.get(), move |scope| {
        let _request_id = request_id.get();
        state.set(XkcdState::Loading);
        let client = http_client.clone();
        scope.launch_background(
            move |_token| async move { fetch_random_comic(&client).await },
            move |result| match result {
                Ok(comic) => state.set(XkcdState::Ready(comic)),
                Err(err) => state.set(XkcdState::Error(err)),
            },
        );
    });

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.11, 0.13, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Text(
                "XKCD Random Comic",
                Modifier::empty()
                    .padding(10.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(14.0),
                TextStyle::default(),
            );

            Text(
                "Downloads a random comic metadata + image from xkcd.com.",
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.14, 0.18, 0.30, 0.8))
                    .rounded_corners(12.0),
                TextStyle::default(),
            );
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    cranpose_ui::Button(
                        Modifier::empty()
                            .padding(10.0)
                            .background(Color(0.21, 0.44, 0.83, 1.0))
                            .rounded_corners(12.0),
                        ButtonSpec::default(),
                        {
                            move || {
                                request_id.update(|value| *value = value.wrapping_add(1));
                            }
                        },
                        || {
                            Text(
                                "Load random comic",
                                Modifier::empty().padding(4.0),
                                TextStyle::default(),
                            );
                        },
                    );
                },
            );

            match state.get() {
                XkcdState::Loading => {
                    Text(
                        "Loading comic...",
                        Modifier::empty()
                            .padding(10.0)
                            .background(Color(0.12, 0.18, 0.30, 0.9))
                            .rounded_corners(12.0),
                        TextStyle::default(),
                    );
                }
                XkcdState::Error(err) => {
                    Text(
                        format!("Error: {err}"),
                        Modifier::empty()
                            .padding(10.0)
                            .background(Color(0.45, 0.18, 0.18, 0.9))
                            .rounded_corners(12.0),
                        TextStyle::default(),
                    );
                }
                XkcdState::Ready(loaded) => {
                    Text(
                        format!("#{} - {}", loaded.comic.num, loaded.comic.title),
                        Modifier::empty()
                            .padding(10.0)
                            .background(Color(0.16, 0.28, 0.18, 0.85))
                            .rounded_corners(12.0),
                        TextStyle::default(),
                    );

                    {
                        let url = format!("https://xkcd.com/{}", loaded.comic.num);
                        let uri_handler = uri_handler.clone();
                        Text(
                            url.clone(),
                            Modifier::empty()
                                .padding(8.0)
                                .background(Color(0.14, 0.18, 0.30, 0.8))
                                .rounded_corners(10.0)
                                .clickable(move |_| {
                                    if let Err(err) = uri_handler.open_uri(&url) {
                                        log::error!("Failed to open xkcd link {}: {:#}", url, err);
                                    }
                                }),
                            TextStyle::default(),
                        );
                    }

                    {
                        let bitmap = loaded.bitmap.clone();
                        let description =
                            Some(format!("xkcd #{} {}", loaded.comic.num, loaded.comic.title));
                        let h_scroll =
                            cranpose_core::remember(|| ScrollState::new(0.0)).with(|s| *s);
                        let v_scroll =
                            cranpose_core::remember(|| ScrollState::new(0.0)).with(|s| *s);
                        Box(
                            Modifier::empty()
                                .fill_max_width()
                                .weight(1.0)
                                .background(Color(0.06, 0.06, 0.06, 1.0))
                                .rounded_corners(14.0)
                                .padding(8.0)
                                .horizontal_scroll(h_scroll, false)
                                .vertical_scroll(v_scroll, false),
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || {
                                Image(
                                    bitmap.clone(),
                                    description.clone(),
                                    Modifier::empty(),
                                    Alignment::CENTER,
                                    ContentScale::None,
                                    1.0,
                                    None,
                                );
                            },
                        );
                    }

                    Spacer(Size::new(0.0, 4.0));
                    Text(
                        loaded.comic.alt,
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(0.10, 0.12, 0.18, 0.85))
                            .rounded_corners(10.0),
                        TextStyle::default(),
                    );
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_u32_never_returns_zero() {
        for _ in 0..100 {
            assert!(random_u32(100) >= 1);
        }
    }

    #[test]
    fn decode_bitmap_rejects_invalid_bytes() {
        assert!(decode_bitmap(&[1, 2, 3, 4]).is_err());
    }
}
