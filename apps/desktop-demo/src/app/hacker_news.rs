#[cfg(test)]
use std::cell::Cell;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use cranpose::LazyItems;
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_core::{self};
use cranpose_foundation::{lazy::LazyListScope, SemanticsConfiguration};
use cranpose_services::{
    isSystemInDarkTheme, local_http_client, local_uri_handler, map_ordered_concurrent,
    HttpClientRef,
};
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle},
    widgets::{BoxWithConstraints, BoxWithConstraintsScope, LazyColumn, LazyColumnSpec},
    Brush, Button, ButtonSpec, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer, LayerShape,
    LinearArrangement, Modifier, RoundedCornerShape, Row, RowSpec, Text, TextStyle,
    VerticalAlignment,
};
use serde::Deserialize;

use super::lazy_scrollbar::{LazyListWithScrollbar, LazyScrollbarStyle};

const PAGE_SIZE: usize = 20;
const AUTOLOAD_THRESHOLD: usize = 1;
const STORY_FETCH_CONCURRENCY: usize = 8;
const COMMENT_PAGE_SIZE: usize = 24;
const COMMENT_FETCH_CONCURRENCY: usize = 8;
const COMMENT_PREFETCH_WINDOW: usize = COMMENT_FETCH_CONCURRENCY * 4;
const COMMENT_AUTOLOAD_THRESHOLD: usize = 2;
const MAX_COMMENT_DEPTH: usize = 6;
const TWO_PANE_BREAKPOINT: f32 = 920.0;
const STORY_LIST_FOOTER_KEY: u64 = 1 << 61;
const THREAD_STORY_KEY: u64 = (1 << 61) + 1;
const THREAD_DISCUSSION_KEY: u64 = (1 << 61) + 2;
const THREAD_FOOTER_KEY: u64 = (1 << 61) + 3;

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Story {
    pub id: u64,
    pub title: Option<String>,
    pub text: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
struct CommentItem {
    id: u64,
    #[serde(default)]
    by: String,
    text: Option<String>,
    #[serde(default)]
    kids: Vec<u64>,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    dead: bool,
    #[serde(default)]
    r#type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommentEntry {
    id: u64,
    depth: usize,
    author: String,
    body: String,
    child_count: usize,
}

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

#[derive(Clone, Debug, PartialEq)]
struct CommentThreadData {
    story: Story,
    comments: Vec<CommentEntry>,
    pager: CommentPager,
    is_loading_more: bool,
    load_more_error: Option<String>,
}

impl CommentThreadData {
    fn new(story: &Story) -> Self {
        let mut pending = Vec::new();
        push_pending_comments(&mut pending, &story.kids, 0);

        Self {
            story: story.clone(),
            comments: Vec::new(),
            pager: CommentPager {
                pending,
                ready_results: HashMap::new(),
                hit_depth_limit: false,
            },
            is_loading_more: false,
            load_more_error: None,
        }
    }

    fn loaded_count(&self) -> usize {
        self.comments.len()
    }

    fn has_more(&self) -> bool {
        !self.pager.pending.is_empty() || !self.pager.ready_results.is_empty()
    }

    fn is_depth_truncated(&self) -> bool {
        self.pager.hit_depth_limit
    }

    fn with_loading_more(mut self, loading: bool) -> Self {
        self.is_loading_more = loading;
        if loading {
            self.load_more_error = None;
        }
        self
    }

    fn with_load_more_error(mut self, message: String) -> Self {
        self.is_loading_more = false;
        self.load_more_error = Some(message);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommentPager {
    pending: Vec<PendingComment>,
    ready_results: HashMap<u64, Result<CommentItem, String>>,
    hit_depth_limit: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum ThreadState {
    Idle,
    Loading(Story),
    Success(CommentThreadData),
    Error { story: Story, message: String },
}

impl ThreadState {
    fn story_id(&self) -> Option<u64> {
        match self {
            ThreadState::Idle => None,
            ThreadState::Loading(story) => Some(story.id),
            ThreadState::Success(data) => Some(data.story.id),
            ThreadState::Error { story, .. } => Some(story.id),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct HackerNewsPalette {
    background: Color,
    header: Color,
    panel: Color,
    surface: Color,
    surface_alt: Color,
    surface_selected: Color,
    primary_text: Color,
    secondary_text: Color,
    accent: Color,
    accent_soft: Color,
    accent_text: Color,
    link: Color,
    error_surface: Color,
    error_text: Color,
}

impl HackerNewsPalette {
    fn new(is_dark: bool) -> Self {
        if is_dark {
            Self {
                background: Color::from_rgb_u8(16, 18, 22),
                header: Color::from_rgb_u8(38, 32, 24),
                panel: Color::from_rgb_u8(24, 27, 33),
                surface: Color::from_rgb_u8(32, 36, 44),
                surface_alt: Color::from_rgb_u8(27, 30, 37),
                surface_selected: Color::from_rgb_u8(48, 46, 38),
                primary_text: Color::from_rgb_u8(236, 239, 244),
                secondary_text: Color::from_rgb_u8(164, 172, 184),
                accent: Color::from_rgb_u8(255, 102, 0),
                accent_soft: Color::from_rgb_u8(73, 54, 38),
                accent_text: Color::from_rgb_u8(255, 246, 235),
                link: Color::from_rgb_u8(125, 191, 255),
                error_surface: Color::from_rgb_u8(74, 31, 34),
                error_text: Color::from_rgb_u8(255, 202, 206),
            }
        } else {
            Self {
                background: Color::from_rgb_u8(244, 242, 236),
                header: Color::from_rgb_u8(255, 102, 0),
                panel: Color::from_rgb_u8(252, 251, 247),
                surface: Color::from_rgb_u8(255, 255, 255),
                surface_alt: Color::from_rgb_u8(248, 246, 240),
                surface_selected: Color::from_rgb_u8(255, 241, 224),
                primary_text: Color::from_rgb_u8(28, 30, 34),
                secondary_text: Color::from_rgb_u8(96, 103, 112),
                accent: Color::from_rgb_u8(255, 102, 0),
                accent_soft: Color::from_rgb_u8(255, 234, 214),
                accent_text: Color::from_rgb_u8(78, 34, 0),
                link: Color::from_rgb_u8(35, 96, 180),
                error_surface: Color::from_rgb_u8(255, 228, 228),
                error_text: Color::from_rgb_u8(165, 33, 43),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingComment {
    id: u64,
    depth: usize,
}

fn page_end(start: usize, total: usize) -> usize {
    (start + PAGE_SIZE).min(total)
}

fn rounded_surface(modifier: Modifier, color: Color, radius: f32) -> Modifier {
    modifier
        .graphics_layer(move || GraphicsLayer {
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(radius)),
            ..Default::default()
        })
        .rounded_corners(radius)
        .draw_behind(move |scope| {
            scope.draw_round_rect(Brush::solid(color), CornerRadii::uniform(radius));
        })
}

fn text_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn bold_text_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
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

fn story_thread_label(story: &Story) -> String {
    let comments = story
        .descendants
        .unwrap_or_else(|| i32::try_from(story.kids.len()).unwrap_or(i32::MAX));
    if comments > 0 {
        format!("View {comments} comments")
    } else {
        "View discussion".to_string()
    }
}

fn comment_child_label(child_count: usize) -> String {
    if child_count == 1 {
        "1 reply".to_string()
    } else {
        format!("{child_count} replies")
    }
}

fn story_body_text(story: &Story) -> String {
    html_to_plain_text(story.text.as_deref().unwrap_or(""))
}

fn comment_entry_from_item(item: &CommentItem, depth: usize) -> Option<CommentEntry> {
    if item.r#type != "comment" {
        return None;
    }

    let body = if item.deleted || item.dead {
        "[deleted]".to_string()
    } else {
        html_to_plain_text(item.text.as_deref().unwrap_or(""))
    };

    if body.is_empty() && item.kids.is_empty() {
        return None;
    }

    Some(CommentEntry {
        id: item.id,
        depth,
        author: if item.by.is_empty() {
            "[anon]".to_string()
        } else {
            item.by.clone()
        },
        body,
        child_count: item.kids.len(),
    })
}

fn html_to_plain_text(html: &str) -> String {
    let mut plain = String::with_capacity(html.len());
    let mut tag = String::new();
    let mut entity = String::new();
    let mut in_tag = false;
    let mut in_entity = false;

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                append_tag_separator(&tag, &mut plain);
                tag.clear();
                in_tag = false;
            } else {
                tag.push(ch);
            }
            continue;
        }

        if in_entity {
            entity.push(ch);
            if ch == ';' {
                plain.push_str(decode_html_entity(&entity));
                entity.clear();
                in_entity = false;
            }
            continue;
        }

        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '&' => {
                in_entity = true;
                entity.clear();
                entity.push(ch);
            }
            _ => plain.push(ch),
        }
    }

    if in_entity {
        plain.push_str(&entity);
    }

    collapse_text_whitespace(&plain)
}

fn append_tag_separator(tag: &str, plain: &mut String) {
    let normalized = tag.trim().to_ascii_lowercase();
    if (normalized.starts_with("br")
        || normalized.starts_with("p")
        || normalized.starts_with("/p")
        || normalized.starts_with("div")
        || normalized.starts_with("/div")
        || normalized.starts_with("pre")
        || normalized.starts_with("/pre"))
        && !plain.ends_with('\n')
        && !plain.is_empty()
    {
        plain.push('\n');
    }
    if normalized.starts_with("li") {
        if !plain.ends_with('\n') && !plain.is_empty() {
            plain.push('\n');
        }
        plain.push_str("• ");
    }
}

fn decode_html_entity(entity: &str) -> &'static str {
    match entity {
        "&amp;" => "&",
        "&gt;" => ">",
        "&lt;" => "<",
        "&quot;" => "\"",
        "&#39;" | "&#x27;" => "'",
        "&#47;" | "&#x2F;" => "/",
        "&nbsp;" => " ",
        _ => "",
    }
}

fn collapse_text_whitespace(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut newline_run = 0usize;

    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                pending_space = false;
                if newline_run < 2 {
                    collapsed.push('\n');
                }
                newline_run += 1;
            }
            c if c.is_whitespace() => {
                if !collapsed.ends_with('\n') {
                    pending_space = true;
                }
            }
            c => {
                if pending_space && !collapsed.is_empty() && !collapsed.ends_with(' ') {
                    collapsed.push(' ');
                }
                pending_space = false;
                newline_run = 0;
                collapsed.push(c);
            }
        }
    }

    collapsed.trim().to_string()
}

async fn fetch_top_story_ids(client: &HttpClientRef) -> Result<Vec<u64>, String> {
    let ids_json = client
        .get_text("https://hacker-news.firebaseio.com/v0/topstories.json")
        .await
        .map_err(|err| format!("Failed to fetch top stories: {err}"))?;
    serde_json::from_str(&ids_json).map_err(|err| format!("Failed to parse top story IDs: {err}"))
}

async fn fetch_story(client: &HttpClientRef, id: u64) -> Result<Story, String> {
    let url = format!("https://hacker-news.firebaseio.com/v0/item/{id}.json");
    let json = client
        .get_text(&url)
        .await
        .map_err(|err| format!("Failed to fetch story {id}: {err}"))?;
    serde_json::from_str::<Story>(&json).map_err(|err| format!("Failed to parse story {id}: {err}"))
}

async fn fetch_comment_item(client: &HttpClientRef, id: u64) -> Result<CommentItem, String> {
    let url = format!("https://hacker-news.firebaseio.com/v0/item/{id}.json");
    let json = client
        .get_text(&url)
        .await
        .map_err(|err| format!("Failed to fetch comment {id}: {err}"))?;
    serde_json::from_str::<CommentItem>(&json)
        .map_err(|err| format!("Failed to parse comment {id}: {err}"))
}

async fn fetch_stories_page(
    client: &HttpClientRef,
    ids: &[u64],
    start: usize,
    end: usize,
) -> Result<Vec<Story>, String> {
    let page_ids = ids
        .iter()
        .copied()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect::<Vec<_>>();
    let results = map_ordered_concurrent(&page_ids, STORY_FETCH_CONCURRENCY, {
        let client = client.clone();
        move |id| {
            let client = client.clone();
            async move { fetch_story(&client, id).await }
        }
    })
    .await
    .map_err(|err| format!("Failed to fetch story batch: {err}"))?;

    Ok(results
        .into_iter()
        .filter_map(|result| match result {
            Ok(story) => Some(story),
            Err(err) => {
                log::warn!("{err}");
                None
            }
        })
        .collect())
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

async fn fetch_comment_items_batch(
    client: &HttpClientRef,
    ids: &[u64],
) -> Vec<Result<CommentItem, String>> {
    match map_ordered_concurrent(ids, COMMENT_FETCH_CONCURRENCY, {
        let client = client.clone();
        move |id| {
            let client = client.clone();
            async move { fetch_comment_item(&client, id).await }
        }
    })
    .await
    {
        Ok(results) => results,
        Err(err) => ids
            .iter()
            .map(|id| Err(format!("Failed to fetch comment batch item {id}: {err}")))
            .collect(),
    }
}

fn push_pending_comments(pending: &mut Vec<PendingComment>, ids: &[u64], depth: usize) {
    pending.extend(
        ids.iter()
            .rev()
            .copied()
            .map(|id| PendingComment { id, depth }),
    );
}

fn select_comment_prefetch_batch(
    pending: &[PendingComment],
    ready_results: &HashMap<u64, Result<CommentItem, String>>,
    target_ready_count: usize,
) -> Vec<u64> {
    let batch_capacity = target_ready_count
        .saturating_sub(ready_results.len())
        .min(COMMENT_PREFETCH_WINDOW);

    if batch_capacity == 0 {
        return Vec::new();
    }

    let mut batch = Vec::with_capacity(batch_capacity);
    for pending_comment in pending.iter().rev() {
        if ready_results.contains_key(&pending_comment.id) || batch.contains(&pending_comment.id) {
            continue;
        }
        batch.push(pending_comment.id);
        if batch.len() == batch_capacity {
            break;
        }
    }
    batch
}

async fn load_initial_comment_page(
    client: &HttpClientRef,
    story: &Story,
) -> Result<CommentThreadData, String> {
    load_comment_page(client, CommentThreadData::new(story), COMMENT_PAGE_SIZE).await
}

async fn load_comment_page(
    client: &HttpClientRef,
    mut data: CommentThreadData,
    page_size: usize,
) -> Result<CommentThreadData, String> {
    let mut appended_count = 0usize;
    let target_ready_count = page_size
        .saturating_add(COMMENT_FETCH_CONCURRENCY)
        .max(COMMENT_FETCH_CONCURRENCY);

    while appended_count < page_size {
        while let Some(next) = data.pager.pending.last().copied() {
            let Some(result) = data.pager.ready_results.remove(&next.id) else {
                break;
            };
            data.pager.pending.pop();

            match result {
                Ok(item) => {
                    if next.depth + 1 >= MAX_COMMENT_DEPTH {
                        if !item.kids.is_empty() {
                            data.pager.hit_depth_limit = true;
                        }
                    } else {
                        push_pending_comments(&mut data.pager.pending, &item.kids, next.depth + 1);
                    }

                    if let Some(entry) = comment_entry_from_item(&item, next.depth) {
                        data.comments.push(entry);
                        appended_count += 1;
                        if appended_count >= page_size {
                            break;
                        }
                    }
                }
                Err(err) => log::warn!("{err}"),
            }
        }

        if appended_count >= page_size {
            break;
        }

        let batch_ids = select_comment_prefetch_batch(
            &data.pager.pending,
            &data.pager.ready_results,
            target_ready_count,
        );
        if batch_ids.is_empty() {
            break;
        }

        let batch_results = fetch_comment_items_batch(client, &batch_ids).await;
        for (id, result) in batch_ids.into_iter().zip(batch_results) {
            data.pager.ready_results.insert(id, result);
        }
    }

    data.is_loading_more = false;
    data.load_more_error = None;
    Ok(data)
}

fn launch_initial_load(
    trigger: u64,
    state: cranpose_core::MutableState<NewsState>,
    client: HttpClientRef,
) {
    cranpose_core::LaunchedEffect(trigger, move |scope| {
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
    cranpose_core::LaunchedEffect(trigger, move |scope| {
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
                            if data.ids != ids || data.next_index != start {
                                return;
                            }
                            *current =
                                NewsState::Success(data.clone().append_page(new_stories, end));
                        }
                    });
                }
                Err(err) => {
                    log::error!("Failed to load more stories: {err}");
                    state.update(|current| {
                        if let NewsState::Success(data) = current {
                            if data.ids != ids || data.next_index != start {
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

fn launch_comment_thread(
    selected_story: Option<Story>,
    refresh_trigger: u64,
    state: cranpose_core::MutableState<ThreadState>,
    client: HttpClientRef,
) {
    let selected_story_id = selected_story.as_ref().map(|story| story.id);

    cranpose_core::LaunchedEffect((selected_story_id, refresh_trigger), move |scope| {
        let Some(story) = selected_story.clone() else {
            state.set(ThreadState::Idle);
            return;
        };

        state.set(ThreadState::Loading(story.clone()));
        let client = client.clone();
        let story_for_load = story.clone();

        scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return Err("Cancelled".to_string());
                }
                load_initial_comment_page(&client, &story_for_load).await
            },
            move |result| match result {
                Ok(data) => state.set(ThreadState::Success(data)),
                Err(message) => state.set(ThreadState::Error { story, message }),
            },
        );
    });
}

fn launch_load_more_comments(
    trigger: u64,
    state: cranpose_core::MutableState<ThreadState>,
    client: HttpClientRef,
) {
    cranpose_core::LaunchedEffect(trigger, move |scope| {
        if trigger == 0 {
            return;
        }

        let data = match state.get() {
            ThreadState::Success(data) => {
                if data.is_loading_more || !data.has_more() {
                    return;
                }
                state.set(ThreadState::Success(data.clone().with_loading_more(true)));
                data
            }
            _ => return,
        };

        let expected_story_id = data.story.id;
        let expected_loaded_count = data.loaded_count();
        let client = client.clone();

        scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return Err("Cancelled".to_string());
                }
                load_comment_page(&client, data, COMMENT_PAGE_SIZE).await
            },
            move |result| match result {
                Ok(updated) => {
                    state.update(|current| {
                        if let ThreadState::Success(current_data) = current {
                            if current_data.story.id != expected_story_id
                                || current_data.loaded_count() != expected_loaded_count
                            {
                                return;
                            }
                            *current = ThreadState::Success(updated.clone());
                        }
                    });
                }
                Err(err) => {
                    log::error!("Failed to load more comments: {err}");
                    state.update(|current| {
                        if let ThreadState::Success(current_data) = current {
                            if current_data.story.id != expected_story_id
                                || current_data.loaded_count() != expected_loaded_count
                            {
                                return;
                            }
                            *current = ThreadState::Success(
                                current_data.clone().with_load_more_error(err.clone()),
                            );
                        }
                    });
                }
            },
        );
    });
}

#[composable]
fn AutoLoadMore(
    list_state: cranpose_foundation::lazy::LazyListState,
    news_state: cranpose_core::MutableState<NewsState>,
    auto_load_guard: cranpose_core::MutableState<usize>,
    load_more_trigger: cranpose_core::MutableState<u64>,
) {
    #[cfg(test)]
    DebugScopeTag("AutoLoadMore");
    let visible_start = list_state.first_visible_item_index();
    let visible_count = list_state.stats().items_in_use;
    let visible_end = visible_start.saturating_add(visible_count.saturating_sub(1));

    let (should_trigger, next_index) = match news_state.get() {
        NewsState::Success(data) => {
            let last_story_index = data.stories.len().saturating_sub(1);
            let preload_index = last_story_index.saturating_sub(AUTOLOAD_THRESHOLD);
            let should = data.has_more()
                && !data.is_loading_more
                && visible_end >= preload_index
                && auto_load_guard.get() != data.next_index;
            (should, data.next_index)
        }
        _ => (false, 0),
    };

    cranpose_core::LaunchedEffect((should_trigger, next_index), move |_scope| {
        if should_trigger {
            auto_load_guard.set(next_index);
            load_more_trigger.update(|value| *value = value.wrapping_add(1));
        }
    });
}

#[composable]
fn AutoLoadMoreComments(
    list_state: cranpose_foundation::lazy::LazyListState,
    thread_data: CommentThreadData,
    auto_load_guard: cranpose_core::MutableState<usize>,
    load_more_trigger: cranpose_core::MutableState<u64>,
) {
    #[cfg(test)]
    DebugScopeTag("AutoLoadMoreComments");
    let visible_start = list_state.first_visible_item_index();
    let visible_count = list_state.stats().items_in_use;
    let visible_end = visible_start.saturating_add(visible_count.saturating_sub(1));
    let footer_index = thread_data.comments.len().saturating_add(1);
    let preload_index = footer_index.saturating_sub(COMMENT_AUTOLOAD_THRESHOLD);
    let loaded_count = thread_data.loaded_count();
    let should_trigger = thread_data.has_more()
        && !thread_data.is_loading_more
        && visible_end >= preload_index
        && auto_load_guard.get() != loaded_count;

    cranpose_core::LaunchedEffect((should_trigger, loaded_count), move |_scope| {
        if should_trigger {
            auto_load_guard.set(loaded_count);
            load_more_trigger.update(|value| *value = value.wrapping_add(1));
        }
    });
}

fn discussion_status_detail(data: &CommentThreadData) -> String {
    let mut detail = format!("{} comments shown", data.loaded_count());

    if data.is_loading_more {
        detail.push_str(". Loading more…");
    } else if data.has_more() {
        detail.push_str(". Scroll near the end to load more.");
    } else {
        detail.push_str(". Reached the current end of the thread.");
    }

    if data.is_depth_truncated() {
        detail.push_str(&format!(
            " Replies deeper than {MAX_COMMENT_DEPTH} levels stay collapsed."
        ));
    }

    detail
}

#[cfg(test)]
thread_local! {
    static DEBUG_SCOPE_TAGS: RefCell<HashMap<usize, &'static str>> = RefCell::new(HashMap::new());
    static STORIES_PANE_CALLS: Cell<usize> = const { Cell::new(0) };
    static THREAD_PANE_CALLS: Cell<usize> = const { Cell::new(0) };
    static LAST_STORIES_PANE_NODE_ID: RefCell<Option<usize>> = const { RefCell::new(None) };
    static LAST_STORIES_LIST_STATE: RefCell<Option<cranpose_foundation::lazy::LazyListState>> = const { RefCell::new(None) };
}

#[cfg(test)]
#[composable]
fn DebugScopeTag(name: &'static str) {
    cranpose_core::with_current_composer(|composer| {
        if let Some(scope) = composer.current_recompose_scope() {
            DEBUG_SCOPE_TAGS.with(|tags| {
                tags.borrow_mut().insert(scope.id(), name);
            });
        }
    });
}

#[composable]
fn ActionButton<F>(label: String, background: Color, text_color: Color, on_click: F)
where
    F: FnMut() + 'static,
{
    #[cfg(test)]
    DebugScopeTag("ActionButton");
    Button(
        rounded_surface(Modifier::empty(), background, 8.0).padding_symmetric(10.0, 6.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(
                label.clone(),
                Modifier::empty().padding(2.0),
                text_style(text_color),
            );
        },
    );
}

fn hacker_news_scrollbar_style(palette: HackerNewsPalette) -> LazyScrollbarStyle {
    LazyScrollbarStyle {
        rail_width: 16.0,
        thumb_width: 8.0,
        min_thumb_height: 32.0,
        rail_color: palette.surface_alt,
        thumb_color: palette.accent.with_alpha(0.9),
    }
}

#[composable]
fn StatusCard(
    modifier: Modifier,
    title: String,
    detail: Option<String>,
    background: Color,
    title_color: Color,
    detail_color: Color,
) {
    #[cfg(test)]
    DebugScopeTag("StatusCard");
    Column(
        rounded_surface(modifier, background, 14.0).padding(14.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
        move || {
            Text(
                title.clone(),
                Modifier::empty(),
                bold_text_style(title_color),
            );
            if let Some(detail) = detail.clone() {
                Text(detail, Modifier::empty(), text_style(detail_color));
            }
        },
    );
}

#[composable]
fn HackerNewsHeader<F1, F2, F3>(
    palette: HackerNewsPalette,
    show_back: bool,
    is_dark: bool,
    on_back: F1,
    on_refresh: F2,
    on_toggle_theme: F3,
) where
    F1: FnMut() + 'static,
    F2: FnMut() + 'static,
    F3: FnMut() + 'static,
{
    #[cfg(test)]
    DebugScopeTag("HackerNewsHeader");
    let on_back: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_back));
    let on_refresh: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_refresh));
    let on_toggle_theme: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_toggle_theme));

    Row(
        rounded_surface(Modifier::empty().fill_max_width(), palette.header, 16.0).padding(14.0),
        RowSpec::new()
            .vertical_alignment(VerticalAlignment::CenterVertically)
            .horizontal_arrangement(LinearArrangement::SpaceBetween),
        move || {
            let on_back_handle = Rc::clone(&on_back);
            let on_refresh_handle = Rc::clone(&on_refresh);
            let on_toggle_theme_handle = Rc::clone(&on_toggle_theme);

            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(2.0)),
                move || {
                    Text(
                        "Hacker News",
                        Modifier::empty(),
                        bold_text_style(Color::WHITE),
                    );
                    Text(
                        "Fast top stories with inline threads.",
                        Modifier::empty(),
                        text_style(Color::from_rgba_u8(255, 255, 255, 220)),
                    );
                },
            );

            Row(
                Modifier::empty(),
                RowSpec::new()
                    .vertical_alignment(VerticalAlignment::CenterVertically)
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    if show_back {
                        ActionButton(
                            "Back".to_string(),
                            Color::from_rgba_u8(255, 255, 255, 46),
                            Color::WHITE,
                            {
                                let on_back = Rc::clone(&on_back_handle);
                                move || (on_back.borrow_mut())()
                            },
                        );
                    }
                    ActionButton(
                        "Refresh".to_string(),
                        Color::from_rgba_u8(255, 255, 255, 38),
                        Color::WHITE,
                        {
                            let on_refresh = Rc::clone(&on_refresh_handle);
                            move || (on_refresh.borrow_mut())()
                        },
                    );
                    ActionButton(
                        if is_dark {
                            "Use Light".to_string()
                        } else {
                            "Use Dark".to_string()
                        },
                        Color::from_rgba_u8(255, 255, 255, 38),
                        Color::WHITE,
                        {
                            let on_toggle_theme = Rc::clone(&on_toggle_theme_handle);
                            move || (on_toggle_theme.borrow_mut())()
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn loading_skeleton_item(palette: HackerNewsPalette) {
    let transition = rememberInfiniteTransition("hn_loading_skeleton");
    let pulse = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::tween(900, Easing::EaseInOut),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        "loading_pulse",
    );
    let alpha = 0.35 + 0.65 * pulse.value();

    Row(
        rounded_surface(Modifier::empty().fill_max_width(), palette.surface, 12.0).padding(12.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Loading more",
                Modifier::empty(),
                text_style(palette.secondary_text.with_alpha(alpha)),
            );
            Text(
                "···",
                Modifier::empty(),
                text_style(palette.secondary_text.with_alpha(alpha)),
            );
        },
    );
}

#[composable]
fn StoryItem<F>(
    story: Story,
    rank: usize,
    is_selected: bool,
    palette: HackerNewsPalette,
    on_select_thread: F,
) where
    F: FnMut() + 'static,
{
    #[cfg(test)]
    DebugScopeTag("StoryItem");
    let on_select_thread: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_select_thread));
    let title = story
        .title
        .clone()
        .unwrap_or_else(|| "[No Title]".to_string());
    let metadata = format!("{} points by {}", story.score, story.by);
    let semantics_id = format!("HackerNewsStory {}", story.id);
    let thread_label = story_thread_label(&story);
    let comments_url = story_comments_url(&story);
    let target_url = story_target_url(&story);
    let uri_handler = local_uri_handler().current();

    Row(
        rounded_surface(
            Modifier::empty().fill_max_width(),
            if is_selected {
                palette.surface_selected
            } else {
                palette.surface
            },
            14.0,
        )
        .semantics({
            move |config: &mut SemanticsConfiguration| {
                config.content_description = Some(semantics_id.clone());
            }
        })
        .clickable({
            let on_select_thread = Rc::clone(&on_select_thread);
            move |_| {
                (on_select_thread.borrow_mut())();
            }
        })
        .padding(14.0),
        RowSpec::new()
            .vertical_alignment(VerticalAlignment::Top)
            .horizontal_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            let on_select_thread_handle = Rc::clone(&on_select_thread);
            Text(
                format!("{rank}."),
                Modifier::empty(),
                text_style(palette.secondary_text),
            );

            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let title = title.clone();
                    let metadata = metadata.clone();
                    let thread_label = thread_label.clone();
                    let comments_url = comments_url.clone();
                    let target_url = target_url.clone();
                    let uri_handler = uri_handler.clone();
                    let on_select_thread_handle = Rc::clone(&on_select_thread_handle);
                    move || {
                        Text(
                            title.clone(),
                            Modifier::empty().clickable({
                                let target_url = target_url.clone();
                                let uri_handler = uri_handler.clone();
                                move |_| {
                                    if let Err(err) = uri_handler.open_uri(&target_url) {
                                        log::error!("Failed to open article {target_url}: {err:#}");
                                    }
                                }
                            }),
                            bold_text_style(palette.primary_text),
                        );
                        Text(
                            metadata.clone(),
                            Modifier::empty(),
                            text_style(palette.secondary_text),
                        );
                        Row(
                            Modifier::empty(),
                            RowSpec::new()
                                .vertical_alignment(VerticalAlignment::CenterVertically)
                                .horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                            {
                                let comments_url = comments_url.clone();
                                let uri_handler = uri_handler.clone();
                                let thread_label_for_button = thread_label.clone();
                                let on_select_thread = Rc::clone(&on_select_thread_handle);
                                move || {
                                    ActionButton(
                                        thread_label_for_button.clone(),
                                        palette.accent_soft,
                                        palette.accent_text,
                                        {
                                            let on_select_thread = Rc::clone(&on_select_thread);
                                            move || (on_select_thread.borrow_mut())()
                                        },
                                    );
                                    Text(
                                        "HN",
                                        Modifier::empty().clickable({
                                            let comments_url = comments_url.clone();
                                            let uri_handler = uri_handler.clone();
                                            move |_| {
                                                if let Err(err) = uri_handler.open_uri(&comments_url)
                                                {
                                                    log::error!(
                                                        "Failed to open HN thread {comments_url}: {err:#}"
                                                    );
                                                }
                                            }
                                        }),
                                        text_style(palette.link),
                                    );
                                }
                            },
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn StoriesPane(
    modifier: Modifier,
    list_state: cranpose_foundation::lazy::LazyListState,
    news_state: NewsState,
    selected_story_id: Option<u64>,
    selected_story_state: cranpose_core::MutableState<Option<Story>>,
    thread_refresh_trigger: cranpose_core::MutableState<u64>,
    palette: HackerNewsPalette,
) {
    #[cfg(test)]
    STORIES_PANE_CALLS.with(|count| count.set(count.get() + 1));
    #[cfg(test)]
    LAST_STORIES_LIST_STATE.with(|slot| *slot.borrow_mut() = Some(list_state));
    #[cfg(test)]
    DebugScopeTag("StoriesPane");
    let status_label = match &news_state {
        NewsState::Idle => "Waiting for data".to_string(),
        NewsState::Loading => "Fetching top stories".to_string(),
        NewsState::Error(_) => "Could not load stories".to_string(),
        NewsState::Success(data) => {
            format!("{} loaded of {}", data.stories.len(), data.ids.len())
        }
    };

    let _node_id = Column(
        rounded_surface(modifier, palette.panel, 18.0).padding(14.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            Text(
                "Top stories",
                Modifier::empty(),
                bold_text_style(palette.primary_text),
            );
            Text(
                status_label.clone(),
                Modifier::empty(),
                text_style(palette.secondary_text),
            );
            let list_news_state = news_state.clone();
            let scrollbar_style = hacker_news_scrollbar_style(palette);

            LazyListWithScrollbar(
                Modifier::empty().fill_max_width().weight(1.0),
                list_state,
                "HackerNewsListScrollbarRail",
                scrollbar_style,
                move || {
                    let list_news_state_for_items = list_news_state.clone();
                    LazyColumn(
                        Modifier::empty().fill_max_size().semantics(
                            |config: &mut SemanticsConfiguration| {
                                config.content_description = Some("HackerNewsList".to_string());
                            },
                        ),
                        list_state,
                        LazyColumnSpec::new()
                            .vertical_arrangement(LinearArrangement::SpacedBy(10.0))
                            .content_padding(4.0, 4.0),
                        move |scope| match list_news_state_for_items.clone() {
                            NewsState::Idle => {
                                scope.item_keyed(Some(0), None, move || {
                                    StatusCard(
                                        Modifier::empty().fill_max_width(),
                                        "Idle".to_string(),
                                        Some(
                                            "The initial request has not started yet.".to_string(),
                                        ),
                                        palette.surface,
                                        palette.primary_text,
                                        palette.secondary_text,
                                    );
                                });
                            }
                            NewsState::Loading => {
                                scope.item_keyed(Some(0), None, move || {
                                    StatusCard(
                                        Modifier::empty().fill_max_width(),
                                        "Loading stories…".to_string(),
                                        Some(
                                            "The top page is fetching in the background."
                                                .to_string(),
                                        ),
                                        palette.surface_alt,
                                        palette.primary_text,
                                        palette.secondary_text,
                                    );
                                });
                            }
                            NewsState::Error(message) => {
                                scope.item_keyed(Some(0), None, move || {
                                    StatusCard(
                                        Modifier::empty().fill_max_width(),
                                        "Story load failed".to_string(),
                                        Some(message.clone()),
                                        palette.error_surface,
                                        palette.error_text,
                                        palette.error_text,
                                    );
                                });
                            }
                            NewsState::Success(data) => {
                                let stories = Arc::new(data.stories.clone());
                                scope.items(
                                    LazyItems::new(stories.len()).key({
                                        let stories = Arc::clone(&stories);
                                        move |index: usize| stories[index].id
                                    }),
                                    {
                                        let stories = Arc::clone(&stories);
                                        move |index| {
                                            let story = stories[index].clone();
                                            let story_for_select = story.clone();
                                            StoryItem(
                                                story,
                                                index + 1,
                                                selected_story_id == Some(story_for_select.id),
                                                palette,
                                                move || {
                                                    if selected_story_state
                                                        .get()
                                                        .as_ref()
                                                        .map(|current| current.id)
                                                        != Some(story_for_select.id)
                                                    {
                                                        selected_story_state
                                                            .set(Some(story_for_select.clone()));
                                                    }
                                                    thread_refresh_trigger.set(0);
                                                },
                                            );
                                        }
                                    },
                                );

                                scope.item_keyed(Some(STORY_LIST_FOOTER_KEY), None, {
                                    move || {
                                        if data.has_more() {
                                            if data.is_loading_more {
                                                loading_skeleton_item(palette);
                                            } else {
                                                StatusCard(
                                                    Modifier::empty().fill_max_width(),
                                                    format!("Loaded {} stories", data.stories.len()),
                                                    Some(
                                                        "Scroll to the end to fetch the next page."
                                                            .to_string(),
                                                    ),
                                                    palette.surface_alt,
                                                    palette.primary_text,
                                                    palette.secondary_text,
                                                );
                                            }
                                        } else {
                                            StatusCard(
                                                Modifier::empty().fill_max_width(),
                                                "No more stories".to_string(),
                                                Some("You reached the end of the current top stories snapshot.".to_string()),
                                                palette.surface_alt,
                                                palette.primary_text,
                                                palette.secondary_text,
                                            );
                                        }
                                    }
                                });
                            }
                        },
                    );
                },
            );
        },
    );
    #[cfg(test)]
    LAST_STORIES_PANE_NODE_ID.with(|slot| *slot.borrow_mut() = Some(_node_id));
}

#[composable]
fn StorySummaryCard(story: Story, palette: HackerNewsPalette) {
    let uri_handler = local_uri_handler().current();
    let title = story
        .title
        .clone()
        .unwrap_or_else(|| "[No Title]".to_string());
    let body = story_body_text(&story);
    let target_url = story_target_url(&story);
    let comments_url = story_comments_url(&story);

    Column(
        rounded_surface(
            Modifier::empty().fill_max_width(),
            palette.surface_alt,
            14.0,
        )
        .padding(14.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                title.clone(),
                Modifier::empty(),
                bold_text_style(palette.primary_text),
            );
            Text(
                format!(
                    "{} points by {}",
                    story.score,
                    if story.by.is_empty() {
                        "[anon]"
                    } else {
                        &story.by
                    }
                ),
                Modifier::empty(),
                text_style(palette.secondary_text),
            );
            if !body.is_empty() {
                Text(
                    body.clone(),
                    Modifier::empty(),
                    text_style(palette.primary_text),
                );
            }
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .vertical_alignment(VerticalAlignment::CenterVertically)
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let target_url = target_url.clone();
                    let comments_url = comments_url.clone();
                    let uri_handler = uri_handler.clone();
                    move || {
                        ActionButton(
                            "Open article".to_string(),
                            palette.accent_soft,
                            palette.accent_text,
                            {
                                let target_url = target_url.clone();
                                let uri_handler = uri_handler.clone();
                                move || {
                                    if let Err(err) = uri_handler.open_uri(&target_url) {
                                        log::error!("Failed to open article {target_url}: {err:#}");
                                    }
                                }
                            },
                        );
                        ActionButton(
                            "Open HN".to_string(),
                            palette.surface,
                            palette.primary_text,
                            {
                                let comments_url = comments_url.clone();
                                let uri_handler = uri_handler.clone();
                                move || {
                                    if let Err(err) = uri_handler.open_uri(&comments_url) {
                                        log::error!(
                                            "Failed to open HN thread {comments_url}: {err:#}"
                                        );
                                    }
                                }
                            },
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn CommentRow(comment: CommentEntry, palette: HackerNewsPalette) {
    let indent = ((comment.depth as f32) * 18.0).min(90.0);
    let author = comment.author.clone();
    let child_count = comment.child_count;
    let body = if comment.body.is_empty() {
        "[empty comment]".to_string()
    } else {
        comment.body
    };

    Column(
        rounded_surface(
            Modifier::empty()
                .fill_max_width()
                .padding_each(indent, 0.0, 0.0, 0.0),
            palette.surface,
            12.0,
        )
        .padding(12.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
        move || {
            let author_text = author.clone();
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .vertical_alignment(VerticalAlignment::CenterVertically)
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(
                        author_text.clone(),
                        Modifier::empty(),
                        bold_text_style(palette.primary_text),
                    );
                    if child_count > 0 {
                        Text(
                            comment_child_label(child_count),
                            Modifier::empty(),
                            text_style(palette.secondary_text),
                        );
                    }
                },
            );
            Text(
                body.clone(),
                Modifier::empty(),
                text_style(palette.primary_text),
            );
        },
    );
}

#[composable]
fn CommentsFooter(
    data: CommentThreadData,
    load_more_trigger: cranpose_core::MutableState<u64>,
    palette: HackerNewsPalette,
) {
    if data.is_loading_more {
        loading_skeleton_item(palette);
        return;
    }

    if let Some(message) = data.load_more_error.clone() {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
            move || {
                StatusCard(
                    Modifier::empty().fill_max_width(),
                    "More comments failed".to_string(),
                    Some(message.clone()),
                    palette.error_surface,
                    palette.error_text,
                    palette.error_text,
                );
                ActionButton(
                    "Retry".to_string(),
                    palette.accent,
                    Color::WHITE,
                    move || {
                        load_more_trigger.update(|value| *value = value.wrapping_add(1));
                    },
                );
            },
        );
        return;
    }

    if data.has_more() || data.is_depth_truncated() {
        StatusCard(
            Modifier::empty().fill_max_width(),
            if data.has_more() {
                "More discussion".to_string()
            } else {
                "Thread depth capped".to_string()
            },
            Some(discussion_status_detail(&data)),
            palette.surface_alt,
            palette.primary_text,
            palette.secondary_text,
        );
    }
}

#[composable]
fn ThreadPane(
    modifier: Modifier,
    selected_story: Option<Story>,
    thread_state: ThreadState,
    thread_refresh_trigger: cranpose_core::MutableState<u64>,
    comment_load_more_trigger: cranpose_core::MutableState<u64>,
    comment_auto_load_guard: cranpose_core::MutableState<usize>,
    palette: HackerNewsPalette,
) {
    #[cfg(test)]
    THREAD_PANE_CALLS.with(|count| count.set(count.get() + 1));
    #[cfg(test)]
    DebugScopeTag("ThreadPane");
    Column(
        rounded_surface(modifier, palette.panel, 18.0).padding(14.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || match selected_story.clone() {
            None => {
                StatusCard(
                    Modifier::empty().fill_max_size(),
                    "Pick a story".to_string(),
                    Some("Select “View comments” to open the discussion pane.".to_string()),
                    palette.surface_alt,
                    palette.primary_text,
                    palette.secondary_text,
                );
            }
            Some(story) => {
                if thread_state.story_id() != Some(story.id) {
                    StatusCard(
                        Modifier::empty().fill_max_size(),
                        "Loading discussion…".to_string(),
                        Some("Fetching the selected thread.".to_string()),
                        palette.surface_alt,
                        palette.primary_text,
                        palette.secondary_text,
                    );
                    return;
                }

                match thread_state.clone() {
                    ThreadState::Idle | ThreadState::Loading(_) => {
                        StatusCard(
                            Modifier::empty().fill_max_size(),
                            "Loading discussion…".to_string(),
                            Some("Fetching the selected thread.".to_string()),
                            palette.surface_alt,
                            palette.primary_text,
                            palette.secondary_text,
                        );
                    }
                    ThreadState::Error { message, .. } => {
                        Column(
                            Modifier::empty().fill_max_size(),
                            ColumnSpec::new()
                                .vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
                            move || {
                                StatusCard(
                                    Modifier::empty().fill_max_width(),
                                    "Discussion load failed".to_string(),
                                    Some(message.clone()),
                                    palette.error_surface,
                                    palette.error_text,
                                    palette.error_text,
                                );
                                ActionButton(
                                    "Retry".to_string(),
                                    palette.accent,
                                    Color::WHITE,
                                    move || {
                                        thread_refresh_trigger
                                            .update(|value| *value = value.wrapping_add(1));
                                    },
                                );
                            },
                        );
                    }
                    ThreadState::Success(data) => {
                        let story_key = story.id;
                        cranpose_core::with_key(&story_key, {
                            move || {
                                let comment_list_state =
                                    cranpose_foundation::lazy::rememberLazyListState();
                                let scrollbar_style = hacker_news_scrollbar_style(palette);
                                AutoLoadMoreComments(
                                    comment_list_state,
                                    data.clone(),
                                    comment_auto_load_guard,
                                    comment_load_more_trigger,
                                );
                                LazyListWithScrollbar(
                                    Modifier::empty().fill_max_size(),
                                    comment_list_state,
                                    "HackerNewsCommentsScrollbarRail",
                                    scrollbar_style,
                                    {
                                        let data = data.clone();
                                        move || {
                                            let comments = Arc::new(data.comments.clone());
                                            LazyColumn(
                                                Modifier::empty().fill_max_size().semantics(
                                                    |config: &mut SemanticsConfiguration| {
                                                        config.content_description = Some(
                                                            "HackerNewsCommentsList".to_string(),
                                                        );
                                                    },
                                                ),
                                                comment_list_state,
                                                LazyColumnSpec::new()
                                                    .vertical_arrangement(
                                                        LinearArrangement::SpacedBy(10.0),
                                                    )
                                                    .content_padding(4.0, 4.0),
                                                {
                                                    let comments = Arc::clone(&comments);
                                                    let data = data.clone();
                                                    let story = story.clone();
                                                    move |scope| {
                                                        scope.item_keyed(
                                                            Some(THREAD_STORY_KEY),
                                                            None,
                                                            {
                                                                let story = story.clone();
                                                                move || {
                                                                    StorySummaryCard(
                                                                        story.clone(),
                                                                        palette,
                                                                    );
                                                                }
                                                            },
                                                        );
                                                        scope.item_keyed(
                                                            Some(THREAD_DISCUSSION_KEY),
                                                            None,
                                                            {
                                                                let data = data.clone();
                                                                move || {
                                                                    StatusCard(
                                                                    Modifier::empty()
                                                                        .fill_max_width(),
                                                                    "Discussion".to_string(),
                                                                    Some(discussion_status_detail(
                                                                        &data,
                                                                    )),
                                                                    palette.surface_alt,
                                                                    palette.primary_text,
                                                                    palette.secondary_text,
                                                                    );
                                                                }
                                                            },
                                                        );
                                                        scope.items(
                                                            LazyItems::new(comments.len()).key({
                                                                let comments =
                                                                    Arc::clone(&comments);
                                                                move |index: usize| {
                                                                    comments[index].id
                                                                }
                                                            }),
                                                            {
                                                                let comments =
                                                                    Arc::clone(&comments);
                                                                move |index| {
                                                                    CommentRow(
                                                                        comments[index].clone(),
                                                                        palette,
                                                                    );
                                                                }
                                                            },
                                                        );
                                                        scope.item_keyed(Some(THREAD_FOOTER_KEY), None, {
                                                            let data = data.clone();
                                                            move || {
                                                                if data.comments.is_empty()
                                                                    && !data.has_more()
                                                                {
                                                                    StatusCard(
                                                                        Modifier::empty()
                                                                            .fill_max_width(),
                                                                        "No comments yet"
                                                                            .to_string(),
                                                                        Some(
                                                                            "This story has no discussion items right now.".to_string(),
                                                                        ),
                                                                        palette.surface_alt,
                                                                        palette.primary_text,
                                                                        palette.secondary_text,
                                                                    );
                                                                } else {
                                                                    CommentsFooter(
                                                                        data.clone(),
                                                                        comment_load_more_trigger,
                                                                        palette,
                                                                    );
                                                                }
                                                            }
                                                        });
                                                    }
                                                },
                                            );
                                        }
                                    },
                                );
                            }
                        });
                    }
                }
            }
        },
    );
}

#[composable]
pub fn HackerNewsTab() {
    #[cfg(test)]
    DebugScopeTag("HackerNewsTab");
    let news_state = cranpose_core::rememberMutableStateOf(|| NewsState::Idle);
    let thread_state = cranpose_core::rememberMutableStateOf(|| ThreadState::Idle);
    let refresh_trigger = cranpose_core::rememberMutableStateOf(|| 0u64);
    let load_more_trigger = cranpose_core::rememberMutableStateOf(|| 0u64);
    let thread_refresh_trigger = cranpose_core::rememberMutableStateOf(|| 0u64);
    let comment_load_more_trigger = cranpose_core::rememberMutableStateOf(|| 0u64);
    let comment_auto_load_guard = cranpose_core::rememberMutableStateOf(|| 0usize);
    let selected_story_state = cranpose_core::rememberMutableStateOf(|| None::<Story>);
    let theme_override = cranpose_core::rememberMutableStateOf(|| None::<bool>);
    let list_state = cranpose_foundation::lazy::rememberLazyListState();
    let auto_load_guard = cranpose_core::rememberMutableStateOf(|| 0usize);
    let http_client = local_http_client().current();

    let selected_story = selected_story_state.get();
    let current_news_state = news_state.get();
    let current_thread_state = thread_state.get();
    let system_dark = isSystemInDarkTheme();
    let is_dark = theme_override.get().unwrap_or(system_dark);
    let palette = HackerNewsPalette::new(is_dark);

    launch_initial_load(refresh_trigger.get(), news_state, http_client.clone());
    launch_load_more(load_more_trigger.get(), news_state, http_client.clone());
    launch_comment_thread(
        selected_story.clone(),
        thread_refresh_trigger.get(),
        thread_state,
        http_client.clone(),
    );
    launch_load_more_comments(
        comment_load_more_trigger.get(),
        thread_state,
        http_client.clone(),
    );
    AutoLoadMore(list_state, news_state, auto_load_guard, load_more_trigger);
    cranpose_core::LaunchedEffect(
        (
            selected_story.as_ref().map(|story| story.id),
            thread_refresh_trigger.get(),
        ),
        move |_scope| {
            comment_auto_load_guard.set(0);
        },
    );

    Column(
        Modifier::empty()
            .fill_max_size()
            .clip_to_bounds()
            .background(palette.background)
            .padding(16.0),
        ColumnSpec::default(),
        move || {
            let selected_story_for_constraints = selected_story.clone();
            let current_news_for_constraints = current_news_state.clone();
            let current_thread_for_constraints = current_thread_state.clone();
            BoxWithConstraints(
                Modifier::empty().fill_max_size().clip_to_bounds(),
                move |scope| {
                    #[cfg(test)]
                    DebugScopeTag("hacker_news_tab_box_content");
                    let is_two_pane = scope.max_width().0 >= TWO_PANE_BREAKPOINT;
                    let list_pane_width = (scope.max_width().0 * 0.38).clamp(320.0, 420.0);
                    let show_back = !is_two_pane && selected_story_for_constraints.is_some();
                    let selected_story_for_layout = selected_story_for_constraints.clone();
                    let current_news_for_layout = current_news_for_constraints.clone();
                    let current_thread_for_layout = current_thread_for_constraints.clone();

                    Column(
                        Modifier::empty().fill_max_size().clip_to_bounds(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
                        move || {
                            #[cfg(test)]
                            cranpose_core::debug_label_current_scope("hacker_news_tab_body");
                            HackerNewsHeader(
                                palette,
                                show_back,
                                is_dark,
                                {
                                    let selected_story_handle = { selected_story_state };
                                    move || {
                                        selected_story_handle.set(None);
                                    }
                                },
                                {
                                    let refresh_trigger_state = { refresh_trigger };
                                    let auto_load_guard_state = { auto_load_guard };
                                    let selected_story_handle = { selected_story_state };
                                    move || {
                                        refresh_trigger_state
                                            .update(|value| *value = value.wrapping_add(1));
                                        auto_load_guard_state.set(0);
                                        selected_story_handle.set(None);
                                    }
                                },
                                {
                                    let theme_override_state = { theme_override };
                                    move || {
                                        theme_override_state.set(Some(!is_dark));
                                    }
                                },
                            );

                            if is_two_pane {
                                #[cfg(test)]
                                cranpose_core::debug_label_current_scope(
                                    "hacker_news_tab_two_pane",
                                );
                                let selected_story_for_row = selected_story_for_layout.clone();
                                let current_news_for_row = current_news_for_layout.clone();
                                let current_thread_for_row = current_thread_for_layout.clone();
                                Row(
                                    Modifier::empty()
                                        .fill_max_width()
                                        .weight(1.0)
                                        .clip_to_bounds(),
                                    RowSpec::new()
                                        .vertical_alignment(VerticalAlignment::Top)
                                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                                    move || {
                                        StoriesPane(
                                            Modifier::empty()
                                                .width(list_pane_width)
                                                .fill_max_height(),
                                            list_state,
                                            current_news_for_row.clone(),
                                            selected_story_for_row.as_ref().map(|story| story.id),
                                            selected_story_state,
                                            thread_refresh_trigger,
                                            palette,
                                        );
                                        ThreadPane(
                                            Modifier::empty().weight(1.0).fill_max_height(),
                                            selected_story_for_row.clone(),
                                            current_thread_for_row.clone(),
                                            thread_refresh_trigger,
                                            comment_load_more_trigger,
                                            comment_auto_load_guard,
                                            palette,
                                        );
                                    },
                                );
                            } else {
                                let single_pane_key =
                                    selected_story_for_layout.as_ref().map(|story| story.id);
                                cranpose_core::with_key(&single_pane_key, {
                                    let selected_story_for_single_pane =
                                        selected_story_for_layout.clone();
                                    let current_thread_for_single_pane =
                                        current_thread_for_layout.clone();
                                    let current_news_for_single_pane =
                                        current_news_for_layout.clone();
                                    move || {
                                        if selected_story_for_single_pane.is_some() {
                                            #[cfg(test)]
                                            cranpose_core::debug_label_current_scope(
                                                "hacker_news_tab_thread_only",
                                            );
                                            ThreadPane(
                                                Modifier::empty().fill_max_width().weight(1.0),
                                                selected_story_for_single_pane.clone(),
                                                current_thread_for_single_pane.clone(),
                                                thread_refresh_trigger,
                                                comment_load_more_trigger,
                                                comment_auto_load_guard,
                                                palette,
                                            );
                                        } else {
                                            #[cfg(test)]
                                            cranpose_core::debug_label_current_scope(
                                                "hacker_news_tab_stories_only",
                                            );
                                            StoriesPane(
                                                Modifier::empty().fill_max_width().weight(1.0),
                                                list_state,
                                                current_news_for_single_pane.clone(),
                                                None,
                                                selected_story_state,
                                                thread_refresh_trigger,
                                                palette,
                                            );
                                        }
                                    }
                                });
                            }
                        },
                    );
                },
            );
        },
    );
}

pub const HACKER_NEWS_SCROLL_STABILITY_TARGET_TITLE: &str = "Robot HN Story 024";

#[composable]
pub fn HackerNewsScrollStabilityFixtureTab() {
    let list_state = cranpose_foundation::lazy::rememberLazyListState();
    let selected_story_state = cranpose_core::rememberMutableStateOf(|| None::<Story>);
    let thread_refresh_trigger = cranpose_core::rememberMutableStateOf(|| 0u64);
    let palette = HackerNewsPalette::new(true);
    let news_state = NewsState::Success(scroll_stability_news_data());

    Column(
        Modifier::empty()
            .fill_max_size()
            .clip_to_bounds()
            .background(palette.background)
            .padding(16.0),
        ColumnSpec::default(),
        move || {
            StoriesPane(
                Modifier::empty().fill_max_size(),
                list_state,
                news_state.clone(),
                None,
                selected_story_state,
                thread_refresh_trigger,
                palette,
            );
        },
    );
}

fn scroll_stability_news_data() -> NewsData {
    let stories = (1usize..=72)
        .map(|index| Story {
            id: 9_000_000 + index as u64,
            title: Some(format!("Robot HN Story {index:03}")),
            text: Some(format!(
                "<p>Stable story body {index:03}. Pixel movement in this paragraph must remain locked to the title, metadata, card background, and action row during exact scroll steps.</p>"
            )),
            by: format!("robot-{index:03}"),
            score: 100 + index as i32,
            time: 1_700_000_000 + index as i64 * 60,
            url: Some(format!("https://example.com/robot-hn-story-{index:03}")),
            descendants: Some((index % 17) as i32),
            kids: Vec::new(),
            r#type: "story".to_string(),
        })
        .collect::<Vec<_>>();
    let ids = stories.iter().map(|story| story.id).collect::<Vec<_>>();
    let next_index = ids.len();
    NewsData::new(ids, stories, next_index)
}

#[cfg(test)]
#[path = "tests/hacker_news_tests.rs"]
mod tests;
