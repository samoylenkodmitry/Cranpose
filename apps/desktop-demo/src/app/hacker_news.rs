use cranpose_animation::{animateFloatAsStateWithSpec, AnimationSpec, AnimationType, Easing};
use cranpose_core::{self};
use cranpose_foundation::{lazy::LazyListScope, SemanticsConfiguration};
use cranpose_ui::{
    composable, local_http_client, local_uri_handler,
    text::FontWeight,
    widgets::{LazyColumn, LazyColumnSpec},
    Brush, Button, Color, Column, ColumnSpec, CornerRadii, HttpClientRef, LinearArrangement,
    Modifier, Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
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

const PAGE_SIZE: usize = 20;
const AUTOLOAD_THRESHOLD: usize = 5;

#[derive(Clone, Debug, PartialEq)]
struct NewsData {
    ids: Vec<u64>,
    stories: Vec<Story>,
    next_index: usize,
    is_loading_more: bool,
}

impl NewsData {
    fn new(ids: Vec<u64>, stories: Vec<Story>, next_index: usize) -> Self {
        Self {
            ids,
            stories,
            next_index,
            is_loading_more: false,
        }
    }

    fn has_more(&self) -> bool {
        self.next_index < self.ids.len()
    }

    fn with_loading_more(mut self, loading: bool) -> Self {
        self.is_loading_more = loading;
        self
    }

    fn append_page(mut self, stories: Vec<Story>, next_index: usize) -> Self {
        self.stories.extend(stories);
        self.next_index = next_index;
        self.is_loading_more = false;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
enum NewsState {
    Idle,
    Loading,
    Success(NewsData),
    Error(String),
}

async fn fetch_top_story_ids(client: &HttpClientRef) -> Result<Vec<u64>, String> {
    let ids_json = client
        .get_text("https://hacker-news.firebaseio.com/v0/topstories.json")
        .await
        .map_err(|err| format!("Failed to fetch top stories: {}", err))?;
    serde_json::from_str(&ids_json).map_err(|e| format!("Failed to parse top stories IDs: {}", e))
}

async fn fetch_story(client: &HttpClientRef, id: u64) -> Result<Story, String> {
    let url = format!("https://hacker-news.firebaseio.com/v0/item/{}.json", id);
    let json = client
        .get_text(&url)
        .await
        .map_err(|err| format!("Failed to fetch story {}: {}", id, err))?;
    serde_json::from_str::<Story>(&json).map_err(|e| format!("Failed to parse story {}: {}", id, e))
}

async fn fetch_stories_page(
    client: &HttpClientRef,
    ids: &[u64],
    start: usize,
    end: usize,
) -> Result<Vec<Story>, String> {
    let mut stories = Vec::new();
    for id in ids.iter().skip(start).take(end.saturating_sub(start)) {
        match fetch_story(client, *id).await {
            Ok(story) => stories.push(story),
            Err(_) => continue,
        }
    }
    Ok(stories)
}

fn page_end(start: usize, total: usize) -> usize {
    (start + PAGE_SIZE).min(total)
}

async fn load_initial_page(client: &HttpClientRef) -> Result<NewsData, String> {
    let ids = fetch_top_story_ids(client).await?;
    let end = page_end(0, ids.len());
    let stories = fetch_stories_page(client, &ids, 0, end).await?;
    Ok(NewsData::new(ids, stories, end))
}

async fn load_more_page(
    client: &HttpClientRef,
    ids: Vec<u64>,
    start: usize,
    end: usize,
) -> Result<Vec<Story>, String> {
    fetch_stories_page(client, &ids, start, end).await
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

fn launch_initial_load(
    trigger: u64,
    state: cranpose_core::MutableState<NewsState>,
    client: HttpClientRef,
) {
    cranpose_core::LaunchedEffect!(trigger, move |scope| {
        state.set(NewsState::Loading);
        let client = client.clone();

        scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return Err("Cancelled".to_string());
                }
                load_initial_page(&client).await
            },
            move |result| match result {
                Ok(data) => state.set(NewsState::Success(data)),
                Err(err) => state.set(NewsState::Error(err)),
            },
        );
    });
}

fn launch_load_more(
    trigger: u64,
    state: cranpose_core::MutableState<NewsState>,
    client: HttpClientRef,
) {
    cranpose_core::LaunchedEffect!(trigger, move |scope| {
        if trigger == 0 {
            return;
        }

        let (ids, start, end) = match state.get() {
            NewsState::Success(data) => {
                if data.is_loading_more || !data.has_more() {
                    return;
                }
                let start = data.next_index;
                let end = page_end(start, data.ids.len());
                let ids = data.ids.clone();
                state.set(NewsState::Success(data.with_loading_more(true)));
                (ids, start, end)
            }
            _ => return,
        };

        let ids_for_task = ids.clone();
        let ids_for_result = ids;
        let client = client.clone();
        scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return Err("Cancelled".to_string());
                }
                load_more_page(&client, ids_for_task, start, end).await
            },
            move |result| match result {
                Ok(new_stories) => {
                    state.update(|current| {
                        if let NewsState::Success(data) = current {
                            if data.ids != ids_for_result || data.next_index != start {
                                return;
                            }
                            let updated = data.clone().append_page(new_stories, end);
                            *current = NewsState::Success(updated);
                        }
                    });
                }
                Err(err) => {
                    log::error!("Failed to load more stories: {}", err);
                    state.update(|current| {
                        if let NewsState::Success(data) = current {
                            if data.ids != ids_for_result || data.next_index != start {
                                return;
                            }
                            *current = NewsState::Success(data.clone().with_loading_more(false));
                        }
                    });
                }
            },
        );
    });
}

#[composable]
pub fn hacker_news_tab() {
    let news_state = cranpose_core::useState(|| NewsState::Idle);
    let refresh_trigger = cranpose_core::useState(|| 0u64);
    let load_more_trigger = cranpose_core::useState(|| 0u64);
    let list_state = cranpose_foundation::lazy::remember_lazy_list_state();
    let auto_load_guard = cranpose_core::useState(|| 0usize);
    let http_client = local_http_client().current();

    launch_initial_load(refresh_trigger.get(), news_state, http_client.clone());
    launch_load_more(load_more_trigger.get(), news_state, http_client.clone());

    AutoLoadMore(list_state, news_state, auto_load_guard, load_more_trigger);

    LazyColumn(
        Modifier::empty()
            .fill_max_size()
            .semantics(|config: &mut SemanticsConfiguration| {
                config.content_description = Some("HackerNewsList".to_string());
            })
            .padding(16.0)
            .background(Color(0.96, 0.96, 0.94, 1.0)),
        list_state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move |scope| {
            scope.item(Some(0), None, {
                let trigger_state = refresh_trigger;
                let auto_guard = auto_load_guard;
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
                        move || {
                            Row(
                                Modifier::empty(),
                                RowSpec::new()
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
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
                                    auto_guard.set(0);
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
                        },
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 16.0,
                    });
                }
            });

            match news_state.get() {
                NewsState::Idle => {
                    scope.item(Some(1), None, || {
                        Text(
                            "Status: Idle",
                            Modifier::empty().padding(8.0),
                            TextStyle {
                                color: Some(Color(0.2, 0.2, 0.2, 1.0)),
                                ..Default::default()
                            },
                        );
                    });
                }
                NewsState::Loading => {
                    scope.item(Some(1), None, || {
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
                    });
                }
                NewsState::Error(error) => {
                    let error_message = error.clone();
                    scope.item(Some(1), None, move || {
                        Text(
                            format!("Error: {}", error_message),
                            Modifier::empty()
                                .padding(8.0)
                                .background(Color(1.0, 0.8, 0.8, 1.0)),
                            TextStyle {
                                color: Some(Color(0.8, 0.0, 0.0, 1.0)),
                                ..Default::default()
                            },
                        );
                    });

                    scope.item(Some(2), None, {
                        let trigger = refresh_trigger;
                        let auto_guard = auto_load_guard;
                        move || {
                            Button(
                                Modifier::empty()
                                    .padding(8.0)
                                    .background(Color(0.8, 0.8, 0.8, 1.0))
                                    .rounded_corners(4.0),
                                move || {
                                    trigger.update(|v| *v = v.wrapping_add(1));
                                    auto_guard.set(0);
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
                    });
                }
                NewsState::Success(data) => {
                    let stories = data.stories.clone();
                    scope.items(
                        stories.len(),
                        None::<fn(usize) -> u64>,
                        None::<fn(usize) -> u64>,
                        move |index| {
                            story_item(stories[index].clone(), index + 1);
                        },
                    );

                    scope.item(Some(u64::MAX), None, {
                        let data = data.clone();
                        move || {
                            if data.has_more() {
                                if data.is_loading_more {
                                    loading_stub_item();
                                } else {
                                    Text(
                                        format!(
                                            "Loaded {} of {} stories",
                                            data.stories.len(),
                                            data.ids.len()
                                        ),
                                        Modifier::empty().padding(8.0),
                                        TextStyle {
                                            color: Some(Color(0.5, 0.5, 0.5, 1.0)),
                                            ..Default::default()
                                        },
                                    );
                                }
                            } else {
                                Text(
                                    "No more stories.",
                                    Modifier::empty().padding(8.0),
                                    TextStyle {
                                        color: Some(Color(0.5, 0.5, 0.5, 1.0)),
                                        ..Default::default()
                                    },
                                );
                            }
                        }
                    });
                }
            }
        },
    );
}

/// Auto-loads additional stories when nearing the end of the visible list.
/// This is isolated to its own composable scope so scroll-driven recomposition
/// does not recompose the LazyColumn content.
#[allow(non_snake_case)]
#[composable]
fn AutoLoadMore(
    list_state: cranpose_foundation::lazy::LazyListState,
    news_state: cranpose_core::MutableState<NewsState>,
    auto_load_guard: cranpose_core::MutableState<usize>,
    load_more_trigger: cranpose_core::MutableState<u64>,
) {
    let visible_start = list_state.first_visible_item_index();
    let visible_count = list_state.stats().items_in_use;
    let visible_end = visible_start.saturating_add(visible_count.saturating_sub(1));

    let (should_trigger, next_index) = match news_state.get() {
        NewsState::Success(data) => {
            let last_story_index = 1usize.saturating_add(data.stories.len().saturating_sub(1));
            let preload_index = last_story_index.saturating_sub(AUTOLOAD_THRESHOLD);
            let should = data.has_more()
                && !data.is_loading_more
                && visible_end >= preload_index
                && auto_load_guard.get() != data.next_index;
            (should, data.next_index)
        }
        _ => (false, 0),
    };

    cranpose_core::LaunchedEffect!((should_trigger, next_index), move |_scope| {
        if should_trigger {
            auto_load_guard.set(next_index);
            load_more_trigger.update(|v| *v = v.wrapping_add(1));
        }
    });
}

#[composable]
fn loading_stub_item() {
    let target = cranpose_core::useState(|| 1.0f32);

    cranpose_core::LaunchedEffectAsync!((), move |scope| {
        let target = target;
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            let mut last = clock.next_frame().await;
            let mut elapsed_ms = 0.0f32;
            let mut forward = true;
            let period_ms = 900.0f32;

            while scope.is_active() {
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                let delta_ms = (now.saturating_sub(last)) as f32 / 1_000_000.0;
                last = now;
                elapsed_ms += delta_ms;

                if elapsed_ms >= period_ms {
                    elapsed_ms = 0.0;
                    forward = !forward;
                    target.set(if forward { 1.0 } else { 0.0 });
                }
            }
        })
    });

    let pulse = animateFloatAsStateWithSpec(
        target.get(),
        AnimationType::Tween(AnimationSpec::tween(900, Easing::EaseInOut)),
        "loading_pulse",
    );
    let alpha = 0.35 + 0.65 * pulse.value();

    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(Color(1.0, 1.0, 1.0, 0.9))
            .rounded_corners(6.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Loading more",
                Modifier::empty().padding(2.0),
                TextStyle {
                    color: Some(Color(0.25, 0.25, 0.25, alpha)),
                    ..Default::default()
                },
            );
            Text(
                "···",
                Modifier::empty().padding(2.0),
                TextStyle {
                    color: Some(Color(0.25, 0.25, 0.25, alpha)),
                    ..Default::default()
                },
            );
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
                            font_weight: Some(FontWeight::BOLD),
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
