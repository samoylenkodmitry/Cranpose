use std::{
    cell::Cell,
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
    },
    time::Duration,
};

use cranpose_core::{run_in_mutable_snapshot, CompositionLocalProvider};
use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use cranpose_services::HttpClientRef;
use cranpose_testing::robot::{create_headless_robot_test, RobotTestRule, TestRenderer};
use cranpose_ui::{LayoutBox, SemanticsAction, SemanticsNode, SemanticsRole};
use serde_json::json;

use super::{
    fetch_stories_page, html_to_plain_text, load_comment_page, load_initial_comment_page,
    story_comments_url, story_target_url, CommentThreadData, HackerNewsTab, Story,
};

#[cfg(not(target_arch = "wasm32"))]
fn test_guard() -> MutexGuard<'static, ()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[cfg(not(target_arch = "wasm32"))]
struct RequestConcurrencyTracker {
    active_requests: AtomicUsize,
    max_active_requests: AtomicUsize,
}

#[cfg(not(target_arch = "wasm32"))]
impl RequestConcurrencyTracker {
    fn new() -> Self {
        Self {
            active_requests: AtomicUsize::new(0),
            max_active_requests: AtomicUsize::new(0),
        }
    }

    fn max_active_requests(&self) -> usize {
        self.max_active_requests.load(Ordering::SeqCst)
    }

    fn record_request_start(&self) {
        let active = self.active_requests.fetch_add(1, Ordering::SeqCst) + 1;
        let _ =
            self.max_active_requests
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                    (active > current).then_some(active)
                });
    }

    fn record_request_end(&self) {
        self.active_requests.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct TrackingHttpClient {
    tracker: RequestConcurrencyTracker,
}

#[cfg(not(target_arch = "wasm32"))]
impl TrackingHttpClient {
    fn new() -> Self {
        Self {
            tracker: RequestConcurrencyTracker::new(),
        }
    }

    fn max_active_requests(&self) -> usize {
        self.tracker.max_active_requests()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TrackingHttpClient {
    fn as_client(self: &Arc<Self>) -> HttpClientRef {
        let inner = Arc::clone(self);
        Arc::new(cranpose_services::StubHttpClient::from_text(move |url| {
            inner.text_for(url)
        }))
    }

    fn text_for(&self, url: &str) -> Result<String, cranpose_services::HttpError> {
        let id = parse_story_id(url);
        self.tracker.record_request_start();
        std::thread::sleep(Duration::from_millis(20 + (5 * (id % 3))));
        self.tracker.record_request_end();
        Ok(format!(
            r#"{{"id":{id},"title":"Story {id}","by":"user{id}","score":{id},"time":0,"kids":[],"type":"story"}}"#
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct CommentThreadHttpClient {
    tracker: RequestConcurrencyTracker,
    responses: HashMap<u64, String>,
    latency_ms: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl CommentThreadHttpClient {
    fn new(responses: HashMap<u64, String>, latency_ms: u64) -> Self {
        Self {
            tracker: RequestConcurrencyTracker::new(),
            responses,
            latency_ms,
        }
    }

    fn max_active_requests(&self) -> usize {
        self.tracker.max_active_requests()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl CommentThreadHttpClient {
    fn as_client(self: &Arc<Self>) -> HttpClientRef {
        let inner = Arc::clone(self);
        Arc::new(cranpose_services::StubHttpClient::from_text(move |url| {
            inner.text_for(url)
        }))
    }

    fn text_for(&self, url: &str) -> Result<String, cranpose_services::HttpError> {
        let id = parse_story_id(url);
        self.tracker.record_request_start();
        std::thread::sleep(Duration::from_millis(self.latency_ms));
        self.tracker.record_request_end();
        self.responses.get(&id).cloned().ok_or_else(|| {
            cranpose_services::HttpError::RequestFailed {
                url: url.to_string(),
                message: format!("Missing comment payload for {id}"),
            }
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_story_id(url: &str) -> u64 {
    url.rsplit('/')
        .next()
        .expect("item url suffix")
        .strip_suffix(".json")
        .expect("json suffix")
        .parse()
        .expect("numeric story id")
}

#[cfg(not(target_arch = "wasm32"))]
fn comment_json(id: u64, by: &str, text: &str, kids: &[u64]) -> String {
    json!({
        "id": id,
        "by": by,
        "text": text,
        "kids": kids,
        "type": "comment"
    })
    .to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn comment_ids(data: &CommentThreadData) -> Vec<u64> {
    data.comments.iter().map(|comment| comment.id).collect()
}

#[cfg(not(target_arch = "wasm32"))]
const REGRESSION_MOCK_COMMENT_COUNT: usize = 40;

#[cfg(not(target_arch = "wasm32"))]
struct RegressionHttpClient {
    ids: Vec<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl RegressionHttpClient {
    fn new() -> Self {
        Self::new_with_story_count(3)
    }

    fn new_with_story_count(story_count: usize) -> Self {
        Self {
            ids: (0..story_count)
                .map(|index| 900_001 + index as u64)
                .collect(),
        }
    }

    fn topstories_json(&self) -> String {
        json!(self.ids).to_string()
    }

    fn story_json(&self, id: u64) -> String {
        let index = self
            .ids
            .iter()
            .position(|candidate| *candidate == id)
            .expect("story id should be known");
        let comment_ids = (1..=REGRESSION_MOCK_COMMENT_COUNT)
            .map(|suffix| id * 100 + suffix as u64)
            .collect::<Vec<_>>();
        json!({
            "id": id,
            "title": format!("Regression Story #{}", index + 1),
            "text": format!(
                "<p>{}</p>",
                "A deterministic thread payload used to reproduce the Hacker News back-navigation redraw leak.".repeat(2)
            ),
            "by": "regression-bot",
            "score": 100 + index as i32,
            "time": 1_700_000_000 + index as i64 * 60,
            "url": format!("https://example.com/story/{id}"),
            "descendants": REGRESSION_MOCK_COMMENT_COUNT,
            "kids": comment_ids,
            "type": "story"
        })
        .to_string()
    }

    fn comment_json(&self, id: u64) -> Option<String> {
        let story_id = id / 100;
        let suffix = id % 100;
        if !self.ids.contains(&story_id) {
            return None;
        }
        if suffix == 0 || suffix > REGRESSION_MOCK_COMMENT_COUNT as u64 {
            return None;
        }

        Some(
            json!({
                "id": id,
                "by": format!("commenter-{suffix}"),
                "text": format!(
                    "Regression comment #{suffix}. {}",
                    "This body is long enough to exercise the comments lazy list path without relying on network state.".repeat((suffix as usize % 3) + 1)
                ),
                "kids": [],
                "type": "comment"
            })
            .to_string(),
        )
    }

    fn parse_item_id(url: &str) -> Option<u64> {
        let suffix = url.split("/item/").nth(1)?;
        let id_str = suffix.strip_suffix(".json")?;
        id_str.parse::<u64>().ok()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl RegressionHttpClient {
    fn as_client(self: &Arc<Self>) -> HttpClientRef {
        let inner = Arc::clone(self);
        Arc::new(cranpose_services::StubHttpClient::from_text(move |url| {
            inner.text_for(url)
        }))
    }

    fn text_for(&self, url: &str) -> Result<String, cranpose_services::HttpError> {
        if url.ends_with("/topstories.json") {
            return Ok(self.topstories_json());
        }
        let Some(id) = Self::parse_item_id(url) else {
            return Err(cranpose_services::HttpError::RequestFailed {
                url: url.to_string(),
                message: "unknown mock endpoint".to_string(),
            });
        };
        if let Some(payload) = self.comment_json(id) {
            Ok(payload)
        } else if self.ids.contains(&id) {
            Ok(self.story_json(id))
        } else {
            Err(cranpose_services::HttpError::RequestFailed {
                url: url.to_string(),
                message: "unknown mock item".to_string(),
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn layout_texts(robot: &mut RobotTestRule<TestRenderer>) -> Vec<String> {
    robot.get_all_text()
}

#[cfg(not(target_arch = "wasm32"))]
fn semantics_node_text(node: &SemanticsNode) -> Option<&str> {
    match &node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => node.description.as_deref(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn semantics_contains_text(node: &SemanticsNode, text: &str) -> bool {
    semantics_node_text(node) == Some(text)
        || node
            .children
            .iter()
            .any(|child| semantics_contains_text(child, text))
}

#[cfg(not(target_arch = "wasm32"))]
fn find_clickable_node_with_text(node: &SemanticsNode, text: &str) -> Option<usize> {
    let has_click = node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }));
    if has_click && semantics_contains_text(node, text) {
        return Some(node.node_id);
    }

    node.children
        .iter()
        .find_map(|child| find_clickable_node_with_text(child, text))
}

#[cfg(not(target_arch = "wasm32"))]
fn find_story_list_layout_box(layout: &LayoutBox) -> Option<&LayoutBox> {
    let slices = layout.node_data.modifier_slices();
    let is_story_list_host = matches!(
        layout.node_data.kind,
        cranpose_ui::LayoutNodeKind::Subcompose
    ) && slices.translated_content_context()
        && !slices.pointer_inputs().is_empty();
    if is_story_list_host {
        return Some(layout);
    }

    layout.children.iter().find_map(find_story_list_layout_box)
}

#[cfg(not(target_arch = "wasm32"))]
fn find_parent_node_id(
    layout: &LayoutBox,
    target_node_id: usize,
    parent_node_id: Option<usize>,
) -> Option<usize> {
    if layout.node_id == target_node_id {
        return parent_node_id;
    }

    layout
        .children
        .iter()
        .find_map(|child| find_parent_node_id(child, target_node_id, Some(layout.node_id)))
}

#[cfg(not(target_arch = "wasm32"))]
fn hacker_news_list_node_id(robot: &mut RobotTestRule<TestRenderer>) -> Option<usize> {
    let stories_pane_node_id = super::LAST_STORIES_PANE_NODE_ID.with(|slot| *slot.borrow())?;
    robot.shell_mut().with_layout_tree(|layout_tree| {
        let layout_tree = layout_tree?;
        let stories_pane = find_layout_box_by_node_id(layout_tree.root(), stories_pane_node_id)?;
        find_story_list_layout_box(stories_pane).map(|layout| layout.node_id)
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn stories_list_state() -> cranpose_foundation::lazy::LazyListState {
    super::LAST_STORIES_LIST_STATE
        .with(|slot| (*slot.borrow()).expect("stories list state should be captured"))
}

#[cfg(not(target_arch = "wasm32"))]
fn stories_list_parent_node_id(robot: &mut RobotTestRule<TestRenderer>) -> Option<usize> {
    let list_node_id = hacker_news_list_node_id(robot)?;
    robot.shell_mut().with_layout_tree(|layout_tree| {
        let layout_tree = layout_tree?;
        find_parent_node_id(layout_tree.root(), list_node_id, None)
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn interesting_slot_groups(
    robot: &mut RobotTestRule<TestRenderer>,
) -> Vec<(usize, &'static str, usize)> {
    robot
        .shell_mut()
        .debug_slot_table_groups()
        .into_iter()
        .filter_map(|(start, _key, scope_id, len)| {
            let label = scope_id
                .and_then(|scope_id| {
                    super::DEBUG_SCOPE_TAGS.with(|tags| {
                        tags.borrow()
                            .get(&scope_id)
                            .copied()
                            .or_else(|| cranpose_core::debug_scope_label(scope_id))
                    })
                })
                .filter(|label| {
                    matches!(
                        *label,
                        "HackerNewsTab"
                            | "hacker_news_tab_box_content"
                            | "hacker_news_tab_body"
                            | "hacker_news_tab_stories_only"
                            | "StoriesPane"
                            | "LazyColumnNode"
                    )
                })?;
            Some((start, label, len))
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn find_live_subcompose_node_by_scope_label(
    robot: &mut RobotTestRule<TestRenderer>,
    label: &'static str,
) -> Option<usize> {
    let live = robot.shell_mut().debug_live_subcompose_scope_ids();
    live.iter()
        .find_map(|(node_id, slot_scopes)| {
            let matches = slot_scopes.iter().any(|(_, scope_ids)| {
                scope_ids.iter().any(|scope_id| {
                    super::DEBUG_SCOPE_TAGS.with(|tags| {
                        tags.borrow()
                            .get(scope_id)
                            .copied()
                            .or_else(|| cranpose_core::debug_scope_label(*scope_id))
                    }) == Some(label)
                })
            });
            matches.then_some(*node_id)
        })
        .or_else(|| live.iter().map(|(node_id, _)| *node_id).min())
}

#[cfg(not(target_arch = "wasm32"))]
fn subcompose_slot_table(
    robot: &mut RobotTestRule<TestRenderer>,
    node_id: usize,
    slot_id: u64,
) -> Vec<cranpose_core::SlotDebugEntry> {
    robot
        .shell_mut()
        .debug_subcompose_slot_table(node_id, slot_id)
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn subcompose_interesting_groups(
    robot: &mut RobotTestRule<TestRenderer>,
    node_id: usize,
    slot_id: u64,
) -> Vec<(usize, &'static str, usize)> {
    robot
        .shell_mut()
        .debug_subcompose_slot_groups(node_id, slot_id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(start, _key, scope_id, len)| {
            let label = scope_id
                .and_then(|scope_id| {
                    super::DEBUG_SCOPE_TAGS.with(|tags| {
                        tags.borrow()
                            .get(&scope_id)
                            .copied()
                            .or_else(|| cranpose_core::debug_scope_label(scope_id))
                    })
                })
                .filter(|label| {
                    matches!(
                        *label,
                        "hacker_news_tab_box_content"
                            | "hacker_news_tab_body"
                            | "hacker_news_tab_stories_only"
                            | "StoriesPane"
                            | "LazyColumnNode"
                    )
                })?;
            Some((start, label, len))
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn subcompose_slot_window_around_node(
    robot: &mut RobotTestRule<TestRenderer>,
    node_id: usize,
    slot_id: u64,
    target_node_id: usize,
    radius: usize,
) -> Vec<cranpose_core::SlotDebugEntry> {
    let slots = subcompose_slot_table(robot, node_id, slot_id);
    let needle = format!("id={target_node_id},");
    let Some(index) = slots.iter().position(|entry| {
        entry.kind == cranpose_core::SlotDebugEntryKind::Node && entry.line.contains(&needle)
    }) else {
        return Vec::new();
    };
    let start = index.saturating_sub(radius);
    let end = (index + radius + 1).min(slots.len());
    slots[start..end].to_vec()
}

#[cfg(not(target_arch = "wasm32"))]
fn slot_window_around_node(
    robot: &mut RobotTestRule<TestRenderer>,
    node_id: usize,
    radius: usize,
) -> Vec<cranpose_core::SlotDebugEntry> {
    let slots = robot.shell_mut().debug_slot_entries();
    let needle = format!("id={node_id},");
    let Some(index) = slots.iter().position(|entry| {
        entry.kind == cranpose_core::SlotDebugEntryKind::Node && entry.line.contains(&needle)
    }) else {
        return Vec::new();
    };
    let start = index.saturating_sub(radius);
    let end = (index + radius + 1).min(slots.len());
    slots[start..end].to_vec()
}

#[cfg(not(target_arch = "wasm32"))]
fn find_layout_box_by_node_id(layout: &LayoutBox, node_id: usize) -> Option<&LayoutBox> {
    if layout.node_id == node_id {
        return Some(layout);
    }

    layout
        .children
        .iter()
        .find_map(|child| find_layout_box_by_node_id(child, node_id))
}

#[cfg(not(target_arch = "wasm32"))]
fn layout_subtree_summary(layout: &LayoutBox) -> Vec<String> {
    fn walk(layout: &LayoutBox, depth: usize, lines: &mut Vec<String>) {
        let semantics = layout
            .node_data
            .semantics()
            .and_then(|config| config.content_description.clone());
        let slices = layout.node_data.modifier_slices();
        lines.push(format!(
            "{:indent$}node={} kind={:?} translated={} pointer_inputs={} semantics={:?}",
            "",
            layout.node_id,
            layout.node_data.kind,
            slices.translated_content_context(),
            slices.pointer_inputs().len(),
            semantics,
            indent = depth * 2,
        ));
        for child in &layout.children {
            walk(child, depth + 1, lines);
        }
    }

    let mut lines = Vec::new();
    walk(layout, 0, &mut lines);
    lines
}

#[cfg(not(target_arch = "wasm32"))]
fn pump_robot_until(
    robot: &mut RobotTestRule<TestRenderer>,
    max_steps: usize,
    predicate: impl Fn(&mut RobotTestRule<TestRenderer>) -> bool,
    context: &str,
) {
    for _ in 0..max_steps {
        robot.shell_mut().update();
        if predicate(robot) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let stories_pane_node_id = super::LAST_STORIES_PANE_NODE_ID.with(|slot| *slot.borrow());
    let stories_pane_bounds =
        stories_pane_node_id.and_then(|node_id| robot.shell_mut().node_layout_bounds(node_id));
    let stories_pane_child_count = stories_pane_node_id.and_then(|node_id| {
        robot.shell_mut().with_layout_tree(|layout_tree| {
            let layout_tree = layout_tree?;
            find_layout_box_by_node_id(layout_tree.root(), node_id)
                .map(|layout| layout.children.len())
        })
    });
    let stories_pane_layout = stories_pane_node_id.and_then(|node_id| {
        robot.shell_mut().with_layout_tree(|layout_tree| {
            let layout_tree = layout_tree?;
            find_layout_box_by_node_id(layout_tree.root(), node_id).map(layout_subtree_summary)
        })
    });

    panic!(
        "{context}; visible_texts={:?} stories_pane_calls={} thread_pane_calls={} stories_pane_node_id={stories_pane_node_id:?} stories_pane_bounds={stories_pane_bounds:?} stories_pane_child_count={stories_pane_child_count:?} stories_pane_layout={stories_pane_layout:?}",
        layout_texts(robot),
        super::STORIES_PANE_CALLS.with(Cell::get),
        super::THREAD_PANE_CALLS.with(Cell::get),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn invoke_click(robot: &mut RobotTestRule<TestRenderer>, node_id: usize) {
    let (x, y, width, height) = robot
        .shell_mut()
        .node_layout_bounds(node_id)
        .unwrap_or_else(|| panic!("missing layout bounds for node {node_id}"));
    let handlers = {
        robot.shell_mut().with_layout_tree(|layout_tree| {
            let layout_tree = layout_tree.expect("layout tree should be available");
            let layout_box = find_layout_box_by_node_id(layout_tree.root(), node_id)
                .unwrap_or_else(|| panic!("missing layout box for node {node_id}"));
            layout_box
                .node_data
                .modifier_slices()
                .pointer_inputs()
                .to_vec()
        })
    };

    assert!(
        !handlers.is_empty(),
        "node {node_id} should expose pointer handlers"
    );

    let local = cranpose_ui::Point {
        x: width * 0.5,
        y: height * 0.5,
    };
    let global = cranpose_ui::Point {
        x: x + width * 0.5,
        y: y + height * 0.5,
    };

    run_in_mutable_snapshot(|| {
        let down = PointerEvent::new(PointerEventKind::Down, local, global)
            .with_buttons(PointerButtons::default().with(PointerButton::Primary));
        for handler in &handlers {
            handler(down.clone());
            if down.is_consumed() {
                break;
            }
        }

        let up = PointerEvent::new(PointerEventKind::Up, local, global);
        for handler in &handlers {
            handler(up.clone());
            if up.is_consumed() {
                break;
            }
        }
    })
    .expect("click should run inside a mutable snapshot");
    robot.shell_mut().update();
}

#[cfg(not(target_arch = "wasm32"))]
fn raw_drag_story_list(
    robot: &mut RobotTestRule<TestRenderer>,
    list_bounds: (f32, f32, f32, f32),
    steps: usize,
) -> Vec<usize> {
    let (list_x, list_y, list_w, list_h) = list_bounds;
    let drag_x = list_x + list_w * 0.5;
    let drag_start_y = list_y + list_h * 0.82;
    let drag_end_y = list_y + list_h * 0.22;
    let mut seen = Vec::new();

    robot.shell_mut().set_cursor(drag_x, drag_start_y);
    robot.shell_mut().update();
    std::thread::sleep(Duration::from_millis(50));
    seen.push(hacker_news_list_node_id(robot).expect("HackerNewsList should exist before drag"));

    robot.shell_mut().pointer_pressed();
    robot.shell_mut().update();
    std::thread::sleep(Duration::from_millis(50));
    seen.push(hacker_news_list_node_id(robot).expect("HackerNewsList should exist after down"));

    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let y = drag_start_y + (drag_end_y - drag_start_y) * t;
        robot.shell_mut().set_cursor(drag_x, y);
        robot.shell_mut().update();
        std::thread::sleep(Duration::from_millis(16));
        seen.push(
            hacker_news_list_node_id(robot).expect("HackerNewsList should exist during drag move"),
        );
    }

    robot.shell_mut().pointer_released();
    robot.shell_mut().update();
    std::thread::sleep(Duration::from_millis(50));
    seen.push(hacker_news_list_node_id(robot).expect("HackerNewsList should exist after drag"));
    seen
}

#[cfg(not(target_arch = "wasm32"))]
fn visible_regression_story_numbers(robot: &mut RobotTestRule<TestRenderer>) -> Vec<usize> {
    let mut numbers = layout_texts(robot)
        .into_iter()
        .filter_map(|text| {
            text.strip_prefix("Regression Story #")
                .and_then(|suffix| suffix.parse::<usize>().ok())
        })
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

#[cfg(not(target_arch = "wasm32"))]
struct ProgrammaticStoryScrollTrace {
    list_host_ids: Vec<usize>,
    stories_pane_ids: Vec<usize>,
    list_parent_ids: Vec<usize>,
    invalid_scope_tags_per_step: Vec<Vec<(usize, Option<&'static str>)>>,
}

#[cfg(not(target_arch = "wasm32"))]
fn programmatic_story_scroll_node_ids(
    robot: &mut RobotTestRule<TestRenderer>,
    delta: f32,
    steps: usize,
    _phase: &str,
) -> ProgrammaticStoryScrollTrace {
    let list_state = stories_list_state();
    let mut list_host_ids = Vec::new();
    let mut stories_pane_ids = Vec::new();
    let mut list_parent_ids = Vec::new();
    let mut invalid_scope_tags_per_step = Vec::new();
    for _ in 0..steps {
        robot
            .shell_mut()
            .debug_enter_app_context(|| list_state.dispatch_scroll_delta(delta));
        let invalid_scope_ids = robot.shell_mut().runtime_handle().debug_invalid_scope_ids();
        let invalid_scope_tags = super::DEBUG_SCOPE_TAGS.with(|tags| {
            invalid_scope_ids
                .iter()
                .map(|scope_id| {
                    let app_tag = tags.borrow().get(scope_id).copied();
                    let framework_tag = cranpose_core::debug_scope_label(*scope_id);
                    (*scope_id, app_tag.or(framework_tag))
                })
                .collect::<Vec<_>>()
        });
        invalid_scope_tags_per_step.push(invalid_scope_tags);
        robot.shell_mut().update();
        robot.wait_for_idle();
        list_host_ids.push(
            hacker_news_list_node_id(robot)
                .expect("HackerNewsList should exist during programmatic scroll"),
        );
        stories_pane_ids.push(
            super::LAST_STORIES_PANE_NODE_ID
                .with(|slot| *slot.borrow())
                .expect("StoriesPane node should exist during programmatic scroll"),
        );
        list_parent_ids.push(stories_list_parent_node_id(robot).expect("list parent should exist"));
    }
    ProgrammaticStoryScrollTrace {
        list_host_ids,
        stories_pane_ids,
        list_parent_ids,
        invalid_scope_tags_per_step,
    }
}

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

#[test]
fn html_to_plain_text_strips_tags_and_entities() {
    assert_eq!(
        html_to_plain_text("Hi &amp; <p>bye</p><li>item</li>"),
        "Hi &\nbye\n• item"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fetch_stories_page_loads_native_items_in_parallel() {
    let client_impl = Arc::new(TrackingHttpClient::new());
    let client: HttpClientRef = client_impl.as_client();
    let ids = vec![11, 22, 33, 44];

    let stories =
        pollster::block_on(fetch_stories_page(&client, &ids, 0, ids.len())).expect("stories");

    let fetched_ids = stories.iter().map(|story| story.id).collect::<Vec<_>>();
    assert_eq!(fetched_ids, ids);
    assert!(
        client_impl.max_active_requests() > 1,
        "expected parallel native fetches, max concurrency was {}",
        client_impl.max_active_requests()
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn load_initial_comment_page_preserves_depth_first_thread_order() {
    let client_impl = Arc::new(CommentThreadHttpClient::new(
        HashMap::from([
            (1, comment_json(1, "root-a", "A", &[11, 12])),
            (11, comment_json(11, "child-a1", "A1", &[])),
            (12, comment_json(12, "child-a2", "A2", &[])),
            (2, comment_json(2, "root-b", "B", &[])),
        ]),
        0,
    ));
    let client: HttpClientRef = client_impl.as_client();
    let story = Story {
        id: 500,
        kids: vec![1, 2],
        ..Story::default()
    };

    let thread = pollster::block_on(load_initial_comment_page(&client, &story)).expect("thread");
    let ordered_ids = thread
        .comments
        .iter()
        .map(|comment| comment.id)
        .collect::<Vec<_>>();
    let ordered_depths = thread
        .comments
        .iter()
        .map(|comment| comment.depth)
        .collect::<Vec<_>>();

    assert_eq!(ordered_ids, vec![1, 11, 12, 2]);
    assert_eq!(ordered_depths, vec![0, 1, 1, 0]);
    assert_eq!(thread.loaded_count(), 4);
    assert!(!thread.has_more());
    assert!(!thread.is_depth_truncated());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn load_initial_comment_page_fetches_comments_in_parallel() {
    let client_impl = Arc::new(CommentThreadHttpClient::new(
        HashMap::from([
            (1, comment_json(1, "root-a", "A", &[11])),
            (2, comment_json(2, "root-b", "B", &[])),
            (3, comment_json(3, "root-c", "C", &[])),
            (4, comment_json(4, "root-d", "D", &[])),
            (11, comment_json(11, "child-a1", "A1", &[])),
        ]),
        20,
    ));
    let client: HttpClientRef = client_impl.as_client();
    let story = Story {
        id: 700,
        kids: vec![1, 2, 3, 4],
        ..Story::default()
    };

    let thread = pollster::block_on(load_initial_comment_page(&client, &story)).expect("thread");

    assert_eq!(thread.comments.len(), 5);
    assert!(
        client_impl.max_active_requests() > 1,
        "expected parallel comment fetches, max concurrency was {}",
        client_impl.max_active_requests()
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn load_comment_page_appends_next_depth_first_batch() {
    let client_impl = Arc::new(CommentThreadHttpClient::new(
        HashMap::from([
            (1, comment_json(1, "root-a", "A", &[11])),
            (11, comment_json(11, "child-a1", "A1", &[111])),
            (111, comment_json(111, "child-a1-1", "A1.1", &[])),
            (2, comment_json(2, "root-b", "B", &[])),
        ]),
        0,
    ));
    let client: HttpClientRef = client_impl.as_client();
    let story = Story {
        id: 701,
        kids: vec![1, 2],
        ..Story::default()
    };

    let thread = pollster::block_on(load_comment_page(
        &client,
        CommentThreadData::new(&story),
        2,
    ))
    .expect("first page");
    assert_eq!(comment_ids(&thread), vec![1, 11]);
    assert!(thread.has_more());

    let thread = pollster::block_on(load_comment_page(&client, thread, 2)).expect("next page");
    assert_eq!(comment_ids(&thread), vec![1, 11, 111, 2]);
    assert!(!thread.has_more());
    assert_eq!(thread.loaded_count(), 4);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn single_pane_back_navigation_settles_after_opening_comments() {
    let _guard = test_guard();
    let mock_client: HttpClientRef = Arc::new(RegressionHttpClient::new()).as_client();

    let mut robot = create_headless_robot_test(390, 844, {
        let mock_client = mock_client.clone();
        move || {
            let local = cranpose_services::local_http_client();
            CompositionLocalProvider(vec![local.provides(mock_client.clone())], move || {
                HackerNewsTab();
            });
        }
    });
    robot.shell_mut().set_semantics_enabled(true);
    eprintln!(
        "slots before pump: {:?}",
        robot
            .shell_mut()
            .debug_slot_entries()
            .into_iter()
            .take(40)
            .collect::<Vec<_>>()
    );

    pump_robot_until(
        &mut robot,
        200,
        |robot| {
            robot.shell_mut().update();
            let Some(root) = robot
                .shell_mut()
                .semantics_tree()
                .map(|tree| tree.root().clone())
            else {
                return false;
            };
            find_clickable_node_with_text(
                &root,
                &format!("View {REGRESSION_MOCK_COMMENT_COUNT} comments"),
            )
            .is_some()
        },
        "comments entry point never appeared",
    );

    let comments_node_id = {
        let root = robot
            .shell_mut()
            .semantics_tree()
            .expect("semantics tree should be present")
            .root()
            .clone();
        find_clickable_node_with_text(
            &root,
            &format!("View {REGRESSION_MOCK_COMMENT_COUNT} comments"),
        )
        .expect("first comments button should be clickable")
    };
    invoke_click(&mut robot, comments_node_id);

    pump_robot_until(
        &mut robot,
        200,
        |robot| layout_texts(robot).iter().any(|text| text == "Back"),
        "back button never appeared after opening the first thread",
    );

    let back_node_id = {
        let root = robot
            .shell_mut()
            .semantics_tree()
            .expect("semantics tree should be present")
            .root()
            .clone();
        find_clickable_node_with_text(&root, "Back").expect("back button should be clickable")
    };
    invoke_click(&mut robot, back_node_id);

    pump_robot_until(
        &mut robot,
        200,
        |robot| {
            let texts = layout_texts(robot);
            texts.iter().any(|text| text == "Top stories")
                && texts.iter().all(|text| text != "Back")
        },
        "back navigation did not restore the story list",
    );

    let mut settled = false;
    for _ in 0..120 {
        robot.shell_mut().update();
        if !robot.shell_mut().needs_redraw() {
            settled = true;
            break;
        }
    }

    let stats = robot.shell_mut().debug_runtime_leak_stats();
    let invalid_scope_ids = robot.shell_mut().runtime_handle().debug_invalid_scope_ids();
    let slot_groups = robot.shell_mut().debug_slot_table_groups();
    let live_subcompose_scope_ids = robot.shell_mut().debug_live_subcompose_scope_ids();
    let invalid_scope_sources = invalid_scope_ids
        .iter()
        .map(|scope_id| {
            (
                *scope_id,
                cranpose_core::debug_scope_invalidation_sources(*scope_id),
            )
        })
        .collect::<Vec<_>>();
    let invalid_scope_tags = super::DEBUG_SCOPE_TAGS.with(|tags| {
        invalid_scope_ids
            .iter()
            .map(|scope_id| {
                let app_tag = tags.borrow().get(scope_id).copied();
                let framework_tag = cranpose_core::debug_scope_label(*scope_id);
                (*scope_id, app_tag.or(framework_tag))
            })
            .collect::<Vec<_>>()
    });
    let invalid_root_groups = slot_groups
        .iter()
        .filter(|(_, _, scope_id, _)| {
            scope_id.is_some_and(|scope_id| invalid_scope_ids.contains(&scope_id))
        })
        .copied()
        .collect::<Vec<_>>();
    assert!(
        settled,
        "shell kept requesting redraws after returning from comments; invalid_scope_ids={invalid_scope_ids:?} invalid_scope_tags={invalid_scope_tags:?} invalid_scope_sources={invalid_scope_sources:?} invalid_root_groups={invalid_root_groups:?} live_subcompose_scope_ids={live_subcompose_scope_ids:?} runtime={:?} pass={:?} texts={:?}",
        stats.runtime_stats,
        stats.pass_stats,
        layout_texts(&mut robot),
    );
    assert_eq!(
        stats.runtime_stats.frame_callbacks_len, 0,
        "back navigation should not leave active frame callbacks behind"
    );
    let pending_repasses = robot.shell_mut().debug_enter_app_context(|| {
        (
            cranpose_ui::has_pending_layout_repasses(),
            cranpose_ui::has_pending_draw_repasses(),
            cranpose_ui::has_pending_pointer_repasses(),
        )
    });
    assert!(
        !pending_repasses.0,
        "back navigation should not leave pending layout repasses behind"
    );
    assert!(
        !pending_repasses.1,
        "back navigation should not leave pending draw repasses behind"
    );
    assert!(
        !pending_repasses.2,
        "back navigation should not leave pending pointer repasses behind"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn restored_single_pane_story_list_keeps_same_host_during_drag() {
    let _guard = test_guard();
    let mock_client: HttpClientRef =
        Arc::new(RegressionHttpClient::new_with_story_count(60)).as_client();

    let mut robot = create_headless_robot_test(390, 844, {
        let mock_client = mock_client.clone();
        move || {
            let local = cranpose_services::local_http_client();
            CompositionLocalProvider(vec![local.provides(mock_client.clone())], move || {
                HackerNewsTab();
            });
        }
    });
    robot.shell_mut().set_semantics_enabled(true);
    eprintln!(
        "programmatic scroll initial groups: {:?}",
        interesting_slot_groups(&mut robot)
    );

    pump_robot_until(
        &mut robot,
        200,
        |robot| {
            robot.shell_mut().update();
            layout_texts(robot)
                .iter()
                .any(|text| text == "Regression Story #1")
        },
        "story list never appeared",
    );

    let initial_list_node_id =
        hacker_news_list_node_id(&mut robot).expect("HackerNewsList should exist");
    let list_bounds = robot
        .shell_mut()
        .node_layout_bounds(initial_list_node_id)
        .expect("HackerNewsList should have layout bounds");

    let comments_node_id = {
        let root = robot
            .shell_mut()
            .semantics_tree()
            .expect("semantics tree should be present")
            .root()
            .clone();
        find_clickable_node_with_text(
            &root,
            &format!("View {REGRESSION_MOCK_COMMENT_COUNT} comments"),
        )
        .expect("comments button should be clickable")
    };
    invoke_click(&mut robot, comments_node_id);

    pump_robot_until(
        &mut robot,
        200,
        |robot| layout_texts(robot).iter().any(|text| text == "Back"),
        "back button never appeared after opening comments",
    );

    let back_node_id = {
        let root = robot
            .shell_mut()
            .semantics_tree()
            .expect("semantics tree should be present")
            .root()
            .clone();
        find_clickable_node_with_text(&root, "Back").expect("back button should be clickable")
    };
    invoke_click(&mut robot, back_node_id);

    pump_robot_until(
        &mut robot,
        200,
        |robot| {
            let texts = layout_texts(robot);
            texts.iter().any(|text| text == "Top stories")
                && texts.iter().all(|text| text != "Back")
                && hacker_news_list_node_id(robot).is_some()
        },
        "story list did not return after Back",
    );

    let restored_list_node_id =
        hacker_news_list_node_id(&mut robot).expect("restored HackerNewsList should exist");
    let seen_node_ids = raw_drag_story_list(&mut robot, list_bounds, 12);
    let mut unique_node_ids = seen_node_ids.clone();
    unique_node_ids.sort_unstable();
    unique_node_ids.dedup();

    assert_eq!(
        unique_node_ids,
        vec![restored_list_node_id],
        "restored HackerNewsList host changed during drag; initial_list_node_id={initial_list_node_id} restored_list_node_id={restored_list_node_id} seen_node_ids={seen_node_ids:?} stories_pane_calls={} thread_pane_calls={} visible_texts={:?}",
        super::STORIES_PANE_CALLS.with(Cell::get),
        super::THREAD_PANE_CALLS.with(Cell::get),
        layout_texts(&mut robot),
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn restored_single_pane_programmatic_scroll_keeps_same_list_host() {
    let _guard = test_guard();
    let mock_client: HttpClientRef =
        Arc::new(RegressionHttpClient::new_with_story_count(60)).as_client();

    let mut robot = create_headless_robot_test(390, 844, {
        let mock_client = mock_client.clone();
        move || {
            let local = cranpose_services::local_http_client();
            CompositionLocalProvider(vec![local.provides(mock_client.clone())], move || {
                HackerNewsTab();
            });
        }
    });
    robot.shell_mut().set_semantics_enabled(true);

    pump_robot_until(
        &mut robot,
        200,
        |robot| {
            layout_texts(robot)
                .iter()
                .any(|text| text == "Regression Story #1")
        },
        "story list never appeared",
    );

    let fresh_list_node_id =
        hacker_news_list_node_id(&mut robot).expect("fresh HackerNewsList should exist");
    let fresh_before = visible_regression_story_numbers(&mut robot);
    let fresh_scroll_trace = programmatic_story_scroll_node_ids(&mut robot, -120.0, 6, "fresh");
    let fresh_after = visible_regression_story_numbers(&mut robot);
    assert!(
        fresh_after
            .iter()
            .copied()
            .min()
            .unwrap_or(0)
            .saturating_sub(fresh_before.iter().copied().min().unwrap_or(0))
            >= 2,
        "fresh programmatic scroll should move the list; before={fresh_before:?} after={fresh_after:?}",
    );
    let expected_fresh_list_host_ids =
        vec![fresh_list_node_id; fresh_scroll_trace.list_host_ids.len()];
    assert_eq!(
        fresh_scroll_trace.list_host_ids,
        expected_fresh_list_host_ids,
        "fresh programmatic scroll changed the list host; node_ids={:?} stories_pane_ids={:?} list_parent_ids={:?}",
        fresh_scroll_trace.list_host_ids,
        fresh_scroll_trace.stories_pane_ids,
        fresh_scroll_trace.list_parent_ids,
    );

    let comments_node_id = {
        let root = robot
            .shell_mut()
            .semantics_tree()
            .expect("semantics tree should be present")
            .root()
            .clone();
        find_clickable_node_with_text(
            &root,
            &format!("View {REGRESSION_MOCK_COMMENT_COUNT} comments"),
        )
        .expect("comments button should be clickable")
    };
    invoke_click(&mut robot, comments_node_id);

    pump_robot_until(
        &mut robot,
        200,
        |robot| layout_texts(robot).iter().any(|text| text == "Back"),
        "back button never appeared after opening comments",
    );

    let back_node_id = {
        let root = robot
            .shell_mut()
            .semantics_tree()
            .expect("semantics tree should be present")
            .root()
            .clone();
        find_clickable_node_with_text(&root, "Back").expect("back button should be clickable")
    };
    invoke_click(&mut robot, back_node_id);

    pump_robot_until(
        &mut robot,
        200,
        |robot| {
            let texts = layout_texts(robot);
            texts.iter().any(|text| text == "Top stories")
                && texts.iter().all(|text| text != "Back")
        },
        "story list did not return after Back",
    );

    let restored_list_node_id =
        hacker_news_list_node_id(&mut robot).expect("restored HackerNewsList should exist");
    let restored_list_parent_node_id =
        stories_list_parent_node_id(&mut robot).expect("restored list parent should exist");
    let box_slot_node_id =
        find_live_subcompose_node_by_scope_label(&mut robot, "BoxWithConstraints.slot(0)")
            .expect("BoxWithConstraints slot host should exist");
    let restored_before = visible_regression_story_numbers(&mut robot);
    let slot_groups_before = interesting_slot_groups(&mut robot);
    let slot_window_before = slot_window_around_node(&mut robot, restored_list_node_id, 12);
    let box_slot_window_before = subcompose_slot_table(&mut robot, box_slot_node_id, 0);
    let box_group_labels_before = subcompose_interesting_groups(&mut robot, box_slot_node_id, 0);
    let box_list_window_before = subcompose_slot_window_around_node(
        &mut robot,
        box_slot_node_id,
        0,
        restored_list_node_id,
        12,
    );
    let restored_stories_pane_node_id = super::LAST_STORIES_PANE_NODE_ID
        .with(|slot| *slot.borrow())
        .expect("restored StoriesPane should exist");
    let stories_pane_layout_before = {
        robot.shell_mut().with_layout_tree(|layout_tree| {
            let layout_tree = layout_tree.expect("layout tree should exist");
            let stories_pane =
                find_layout_box_by_node_id(layout_tree.root(), restored_stories_pane_node_id)
                    .expect("stories pane should exist before scroll");
            layout_subtree_summary(stories_pane)
        })
    };
    let restored_scroll_trace =
        programmatic_story_scroll_node_ids(&mut robot, -120.0, 6, "restored");
    let restored_after = visible_regression_story_numbers(&mut robot);
    let slot_groups_after = interesting_slot_groups(&mut robot);
    let slot_window_after = slot_window_around_node(&mut robot, restored_list_node_id, 12);
    let box_slot_window_after = subcompose_slot_table(&mut robot, box_slot_node_id, 0);
    let box_group_labels_after = subcompose_interesting_groups(&mut robot, box_slot_node_id, 0);
    let box_list_window_after = subcompose_slot_window_around_node(
        &mut robot,
        box_slot_node_id,
        0,
        restored_scroll_trace
            .list_host_ids
            .last()
            .copied()
            .unwrap_or(restored_list_node_id),
        12,
    );
    let stories_pane_layout_after = {
        robot.shell_mut().with_layout_tree(|layout_tree| {
            let layout_tree = layout_tree.expect("layout tree should exist");
            let stories_pane =
                find_layout_box_by_node_id(layout_tree.root(), restored_stories_pane_node_id)
                    .expect("stories pane should exist after scroll");
            layout_subtree_summary(stories_pane)
        })
    };
    assert!(
        restored_after
            .iter()
            .copied()
            .min()
            .unwrap_or(0)
            .saturating_sub(restored_before.iter().copied().min().unwrap_or(0))
            >= 2,
        "restored programmatic scroll should move the list; before={restored_before:?} after={restored_after:?}",
    );
    let expected_restored_list_host_ids =
        vec![restored_list_node_id; restored_scroll_trace.list_host_ids.len()];
    assert_eq!(
        restored_scroll_trace.list_host_ids,
        expected_restored_list_host_ids,
        "restored programmatic scroll changed the list host; node_ids={:?} list_parent_id={restored_list_parent_node_id} list_parent_ids_after={:?} stories_pane_id={restored_stories_pane_node_id} stories_pane_ids_after={:?} restored_invalid_scope_tags={:?} box_slot_node_id={box_slot_node_id} fresh_before={fresh_before:?} fresh_after={fresh_after:?} restored_before={restored_before:?} restored_after={restored_after:?} slot_groups_before={slot_groups_before:?} slot_groups_after={slot_groups_after:?} slot_window_before={slot_window_before:?} slot_window_after={slot_window_after:?} box_group_labels_before={box_group_labels_before:?} box_group_labels_after={box_group_labels_after:?} box_list_window_before={box_list_window_before:?} box_list_window_after={box_list_window_after:?} box_slot_window_before={box_slot_window_before:?} box_slot_window_after={box_slot_window_after:?} stories_pane_layout_before={stories_pane_layout_before:?} stories_pane_layout_after={stories_pane_layout_after:?}",
        restored_scroll_trace.list_host_ids,
        restored_scroll_trace.list_parent_ids,
        restored_scroll_trace.stories_pane_ids,
        restored_scroll_trace.invalid_scope_tags_per_step,
    );
}
