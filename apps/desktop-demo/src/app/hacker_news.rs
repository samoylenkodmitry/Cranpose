use super::external_link::local_uri_handler;
#[cfg(not(target_arch = "wasm32"))]
use cranpose_core::LaunchedEffect;
#[cfg(target_arch = "wasm32")]
use cranpose_core::LaunchedEffectAsync;
use cranpose_core::{self};
use cranpose_foundation::lazy::LazyListScope;
use cranpose_ui::{
    composable,
    widgets::{LazyColumn, LazyColumnSpec},
    Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier, Row,
    RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Story {
    pub id: u64,
    pub title: Option<String>,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub score: i32,
    #[serde(default)]
    pub time: i64,
    pub url: Option<String>,
    pub descendants: Option<i32>,
    #[serde(default)]
    pub kids: Vec<u64>,
    #[serde(default)]
    pub r#type: String,
}

#[derive(Clone, Debug, PartialEq)]
enum NewsState {
    Idle,
    Loading,
    Success(Vec<Story>),
    Error(String),
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_stories_blocking() -> Result<Vec<Story>, String> {
    use reqwest::blocking::Client;
    use std::time::Duration;

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("cranpose-desktop-demo/0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let ids: Vec<u64> = client
        .get("https://hacker-news.firebaseio.com/v0/topstories.json")
        .send()
        .map_err(|e| format!("Failed to fetch top stories: {}", e))?
        .json()
        .map_err(|e| format!("Failed to parse top stories IDs: {}", e))?;

    let mut stories = Vec::new();
    for id in ids.iter().take(20) {
        let url = format!("https://hacker-news.firebaseio.com/v0/item/{}.json", id);
        match client.get(&url).send() {
            Ok(response) => {
                if let Ok(story) = response.json::<Story>() {
                    stories.push(story);
                }
            }
            Err(_) => continue,
        }
    }

    Ok(stories)
}

#[cfg(target_arch = "wasm32")]
async fn fetch_stories_async() -> Result<Vec<Story>, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, RequestMode, Response};

    async fn fetch_url(url: &str) -> Result<String, String> {
        let opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(url, &opts)
            .map_err(|e| format!("Failed to create request: {:?}", e))?;

        let window = web_sys::window().ok_or("No window object")?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Fetch failed: {:?}", e))?;

        let resp: Response = resp_value
            .dyn_into()
            .map_err(|_| "Response is not a Response object")?;

        if !resp.ok() {
            return Err(format!("Request failed with status {}", resp.status()));
        }

        let text_promise = resp
            .text()
            .map_err(|e| format!("Failed to get text: {:?}", e))?;
        let text_value = JsFuture::from(text_promise)
            .await
            .map_err(|e| format!("Failed to read body: {:?}", e))?;

        text_value
            .as_string()
            .ok_or_else(|| "Response body is not a string".to_string())
    }

    let ids_json = fetch_url("https://hacker-news.firebaseio.com/v0/topstories.json").await?;
    let ids: Vec<u64> = serde_json::from_str(&ids_json)
        .map_err(|e| format!("Failed to parse top stories IDs: {}", e))?;

    let mut stories = Vec::new();
    for id in ids.iter().take(20) {
        let url = format!("https://hacker-news.firebaseio.com/v0/item/{}.json", id);
        match fetch_url(&url).await {
            Ok(json) => {
                if let Ok(story) = serde_json::from_str::<Story>(&json) {
                    stories.push(story);
                }
            }
            Err(_) => continue,
        }
    }

    Ok(stories)
}

fn story_target_url(story: &Story) -> String {
    match story.url.as_deref() {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => story_comments_url(story),
    }
}

fn story_comments_url(story: &Story) -> String {
    format!("https://news.ycombinator.com/item?id={}", story.id)
}

#[composable]
pub fn hacker_news_tab() {
    let news_state = cranpose_core::useState(|| NewsState::Idle);
    let refresh_trigger = cranpose_core::useState(|| 0u64);

    #[cfg(not(target_arch = "wasm32"))]
    {
        let state = news_state;
        let trigger = refresh_trigger.get();
        LaunchedEffect!(trigger, move |scope| {
            state.set(NewsState::Loading);

            scope.launch_background(
                move |token| {
                    if token.is_cancelled() {
                        return Err("Cancelled".to_string());
                    }
                    fetch_stories_blocking()
                },
                move |result| match result {
                    Ok(stories) => state.set(NewsState::Success(stories)),
                    Err(e) => state.set(NewsState::Error(e)),
                },
            );
        });
    }

    #[cfg(target_arch = "wasm32")]
    {
        let state = news_state;
        let trigger = refresh_trigger.get();
        LaunchedEffectAsync!(trigger, move |scope| {
            let state = state;
            Box::pin(async move {
                state.set(NewsState::Loading);
                match fetch_stories_async().await {
                    Ok(stories) => {
                        if scope.is_active() {
                            state.set(NewsState::Success(stories));
                        }
                    }
                    Err(e) => {
                        if scope.is_active() {
                            state.set(NewsState::Error(e));
                        }
                    }
                }
            })
        });
    }

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(16.0)
            .background(Color(0.96, 0.96, 0.94, 1.0)),
        ColumnSpec::default(),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .background(Color(1.0, 0.4, 0.0, 1.0))
                    .padding(8.0)
                    .rounded_corners(4.0),
                RowSpec::new()
                    .vertical_alignment(VerticalAlignment::CenterVertically)
                    .horizontal_arrangement(LinearArrangement::SpaceBetween),
                {
                    let trigger_state = refresh_trigger;
                    move || {
                        Row(
                            Modifier::empty(),
                            RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                            || {
                                Text(
                                    "Hacker News",
                                    Modifier::empty().padding(4.0),
                                    TextStyle {
                                        color: Some(Color(1.0, 1.0, 1.0, 1.0)),
                                        ..Default::default()
                                    },
                                );
                            },
                        );

                        Button(
                            Modifier::empty()
                                .rounded_corners(4.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(1.0, 1.0, 1.0, 0.2)),
                                        CornerRadii::uniform(4.0),
                                    );
                                })
                                .padding(6.0),
                            move || {
                                trigger_state.update(|v| *v = v.wrapping_add(1));
                            },
                            || {
                                Text(
                                    "Refresh",
                                    Modifier::empty().padding(2.0),
                                    TextStyle {
                                        color: Some(Color(1.0, 1.0, 1.0, 1.0)),
                                        ..Default::default()
                                    },
                                );
                            },
                        );
                    }
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            match news_state.get() {
                NewsState::Idle => {
                    Text(
                        "Status: Idle",
                        Modifier::empty().padding(8.0),
                        TextStyle {
                            color: Some(Color(0.2, 0.2, 0.2, 1.0)),
                            ..Default::default()
                        },
                    );
                }
                NewsState::Loading => {
                    Column(
                        Modifier::empty().fill_max_width().padding(20.0),
                        ColumnSpec::new().horizontal_alignment(
                            cranpose_ui::HorizontalAlignment::CenterHorizontally,
                        ),
                        || {
                            Text(
                                "Loading stories...",
                                Modifier::empty().padding(8.0),
                                TextStyle {
                                    color: Some(Color(0.2, 0.2, 0.2, 1.0)),
                                    ..Default::default()
                                },
                            );
                            Text(
                                "(... fetching from API ...)",
                                Modifier::empty().padding(4.0),
                                TextStyle {
                                    color: Some(Color(0.5, 0.5, 0.5, 1.0)),
                                    ..Default::default()
                                },
                            );
                        },
                    );
                }
                NewsState::Error(error) => {
                    Text(
                        format!("Error: {}", error),
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(1.0, 0.8, 0.8, 1.0)),
                        TextStyle {
                            color: Some(Color(0.8, 0.0, 0.0, 1.0)),
                            ..Default::default()
                        },
                    );

                    Button(
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(0.8, 0.8, 0.8, 1.0))
                            .rounded_corners(4.0),
                        {
                            let state = refresh_trigger;
                            move || state.update(|v| *v = v.wrapping_add(1))
                        },
                        || {
                            Text(
                                "Retry",
                                Modifier::empty().padding(8.0),
                                TextStyle::default(),
                            );
                        },
                    );
                }
                NewsState::Success(stories) => {
                    let list_state = cranpose_foundation::lazy::remember_lazy_list_state();
                    LazyColumn(
                        Modifier::empty()
                            .semantics(
                                |config: &mut cranpose_foundation::SemanticsConfiguration| {
                                    config.content_description = Some("HackerNewsList".to_string());
                                },
                            )
                            .fill_max_width()
                            .weight(1.0),
                        list_state,
                        LazyColumnSpec::new()
                            .vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move |scope| {
                            let stories_list = stories.clone();
                            scope.items(
                                stories_list.len(),
                                None::<fn(usize) -> u64>,
                                None::<fn(usize) -> u64>,
                                move |index| {
                                    story_item(stories_list[index].clone(), index + 1);
                                },
                            );
                        },
                    );
                }
            }
        },
    );
}

#[composable]
fn story_item(story: Story, rank: usize) {
    let title = story
        .title
        .clone()
        .unwrap_or_else(|| "[No Title]".to_string());
    let by = story.by.clone();
    let score = story.score;
    let comments = story.descendants.unwrap_or(0);
    let target_url = story_target_url(&story);
    let comments_url = story_comments_url(&story);
    let uri_handler = local_uri_handler().current();

    Row(
        Modifier::empty()
            .fill_max_width()
            .background(Color(1.0, 1.0, 1.0, 1.0))
            .padding(12.0)
            .rounded_corners(4.0),
        RowSpec::new().vertical_alignment(VerticalAlignment::Top),
        move || {
            Text(
                format!("{}.", rank),
                Modifier::empty().padding(4.0),
                TextStyle {
                    color: Some(Color(0.5, 0.5, 0.5, 1.0)),
                    ..Default::default()
                },
            );

            Spacer(Size {
                width: 8.0,
                height: 0.0,
            });

            Column(Modifier::empty().weight(1.0), ColumnSpec::default(), {
                let title = title.clone();
                let by = by.clone();
                let target_url = target_url.clone();
                let comments_url = comments_url.clone();
                let uri_handler = uri_handler.clone();
                move || {
                    Text(
                        title.clone(),
                        Modifier::empty().padding(2.0).clickable({
                            let uri_handler = uri_handler.clone();
                            let target_url = target_url.clone();
                            move |_| {
                                if let Err(err) = uri_handler.open_uri(&target_url) {
                                    log::error!("Failed to open story {}: {:#}", target_url, err);
                                }
                            }
                        }),
                        TextStyle {
                            color: Some(Color(0.0, 0.0, 0.0, 0.87)),
                            ..Default::default()
                        },
                    );

                    Row(
                        Modifier::empty(),
                        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                        {
                            let by = by.clone();
                            let comments_url = comments_url.clone();
                            let uri_handler = uri_handler.clone();
                            move || {
                                Text(
                                    format!("{} points by {} |", score, by),
                                    Modifier::empty().padding(2.0),
                                    TextStyle {
                                        color: Some(Color(0.5, 0.5, 0.5, 1.0)),
                                        ..Default::default()
                                    },
                                );
                                Text(
                                    format!("{} comments", comments),
                                    Modifier::empty().padding(2.0).clickable({
                                        let comments_url = comments_url.clone();
                                        let uri_handler = uri_handler.clone();
                                        move |_| {
                                            if let Err(err) = uri_handler.open_uri(&comments_url) {
                                                log::error!(
                                                    "Failed to open comments {}: {:#}",
                                                    comments_url,
                                                    err
                                                );
                                            }
                                        }
                                    }),
                                    TextStyle {
                                        color: Some(Color(0.1, 0.45, 0.85, 1.0)),
                                        ..Default::default()
                                    },
                                );
                            }
                        },
                    );
                }
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{story_comments_url, story_target_url, Story};

    #[test]
    fn story_target_url_prefers_story_url() {
        let story = Story {
            id: 123,
            url: Some("https://example.com/story".to_string()),
            ..Story::default()
        };
        assert_eq!(story_target_url(&story), "https://example.com/story");
    }

    #[test]
    fn story_target_url_falls_back_to_hn_discussion() {
        let story = Story {
            id: 999,
            url: None,
            ..Story::default()
        };
        assert_eq!(
            story_target_url(&story),
            "https://news.ycombinator.com/item?id=999"
        );
    }

    #[test]
    fn story_comments_url_targets_hn_discussion() {
        let story = Story {
            id: 42,
            ..Story::default()
        };
        assert_eq!(
            story_comments_url(&story),
            "https://news.ycombinator.com/item?id=42"
        );
    }
}
