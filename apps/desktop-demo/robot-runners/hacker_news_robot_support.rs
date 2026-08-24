use std::time::Duration;

use cranpose::SemanticElement;
use cranpose_services::{HttpClientRef, HttpError, StubHttpClient};
use cranpose_testing::{
    find_button, find_element_by_text_exact, find_in_semantics, find_text_exact,
    print_semantics_with_bounds,
};
use serde_json::json;

pub const MOCK_STORY_COUNT: usize = 60;
pub const MOCK_COMMENT_COUNT: usize = 40;
pub const LONG_COMMENT_SUFFIX: usize = 3;
pub const INITIAL_COMMENTS_STATUS: &str = "24 comments shown. Scroll near the end to load more.";
pub type Bounds = (f32, f32, f32, f32);
const ROBOT_VIEWPORT_WIDTH: f32 = 390.0;

struct MockHackerNewsClient {
    ids: Vec<u64>,
}

impl MockHackerNewsClient {
    fn new() -> Self {
        Self {
            ids: (0..MOCK_STORY_COUNT)
                .map(|index| 1_000_000 + index as u64)
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
            .unwrap_or(0);
        let comment_ids = (1..=MOCK_COMMENT_COUNT)
            .map(|suffix| id * 100 + suffix as u64)
            .collect::<Vec<_>>();
        json!({
            "id": id,
            "title": format!("Mock Story #{}", index + 1),
            "text": format!(
                "<p>{}</p><p>{}</p><p>{}</p>",
                "This story body is intentionally tall so the list and thread views have enough vertical content for drag-scroll validation.".repeat(2),
                "The raw mouse-event robot test needs deterministic content that can expose a restored-list drag regression.".repeat(2),
                "If the story list stops responding after Back, later items will never become reachable through pointer dragging.".repeat(2),
            ),
            "by": "robot",
            "score": 100 + index as i32,
            "time": 1_700_000_000 + index as i64 * 60,
            "url": format!("https://example.com/story/{}", id),
            "descendants": MOCK_COMMENT_COUNT,
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

        if suffix == 0 || suffix > MOCK_COMMENT_COUNT as u64 {
            return None;
        }

        Some(
            json!({
                "id": id,
                "by": format!("commenter-{suffix}"),
                "text": comment_body_text(suffix as usize),
                "kids": [],
                "type": "comment"
            })
            .to_string(),
        )
    }

    fn parse_story_id(url: &str) -> Option<u64> {
        let suffix = url.split("/item/").nth(1)?;
        let id_str = suffix.strip_suffix(".json")?;
        id_str.parse::<u64>().ok()
    }
}

impl MockHackerNewsClient {
    /// The payload this fixture answers `url` with.
    fn text_for(&self, url: &str) -> Result<String, HttpError> {
        if url.ends_with("/topstories.json") {
            return Ok(self.topstories_json());
        }
        let Some(id) = Self::parse_story_id(url) else {
            return Err(HttpError::RequestFailed {
                url: url.to_string(),
                message: "Unknown mock endpoint".to_string(),
            });
        };
        if let Some(payload) = self.comment_json(id) {
            Ok(payload)
        } else if self.ids.contains(&id) {
            Ok(self.story_json(id))
        } else {
            Err(HttpError::RequestFailed {
                url: url.to_string(),
                message: "Unknown mock item".to_string(),
            })
        }
    }

    /// This fixture as an HTTP client.
    pub(crate) fn into_client(self) -> HttpClientRef {
        std::sync::Arc::new(StubHttpClient::from_text(move |url| self.text_for(url)))
    }
}

fn collect_story_elements<'a>(elem: &'a SemanticElement, stories: &mut Vec<&'a SemanticElement>) {
    if elem
        .text
        .as_deref()
        .is_some_and(|text| text.starts_with("HackerNewsStory "))
    {
        stories.push(elem);
    }
    for child in &elem.children {
        collect_story_elements(child, stories);
    }
}

fn find_mock_story_number(elem: &SemanticElement) -> Option<usize> {
    if let Some(text) = elem.text.as_deref() {
        if let Some(number) = text.strip_prefix("Mock Story #") {
            if let Ok(number) = number.parse::<usize>() {
                return Some(number);
            }
        }
    }
    for child in &elem.children {
        if let Some(number) = find_mock_story_number(child) {
            return Some(number);
        }
    }
    None
}

fn find_comment_number(elem: &SemanticElement) -> Option<usize> {
    if let Some(text) = elem.text.as_deref() {
        if let Some(number) = text.strip_prefix("commenter-") {
            if let Ok(number) = number.parse::<usize>() {
                return Some(number);
            }
        }
    }
    for child in &elem.children {
        if let Some(number) = find_comment_number(child) {
            return Some(number);
        }
    }
    None
}

pub fn create_mock_client() -> HttpClientRef {
    MockHackerNewsClient::new().into_client()
}

pub fn long_comment_body_text() -> String {
    comment_body_text(LONG_COMMENT_SUFFIX)
}

fn comment_body_text(suffix: usize) -> String {
    if suffix == LONG_COMMENT_SUFFIX {
        return format!(
            "Mock comment #{suffix}. {}",
            [
                "This intentionally long comment exercises a retained lazy-list item whose measured height must stay locked to its text layout.",
                "The body wraps across many lines in the narrow single-pane thread view, but it must not leave a large empty tail before the next comment.",
                "A stale cached item height is visible as excessive blank space after this paragraph, so the robot checks the semantic gap to the next author row.",
                "The text repeats enough natural words to force wrapping without relying on remote Hacker News content or timing-sensitive network state."
            ].join(" ")
        );
    }
    format!(
        "Mock comment #{suffix}. {}",
        "This body is long enough to produce a realistic multi-line row in the discussion view."
            .repeat((suffix % 3) + 1)
    )
}

fn bounds_center(bounds: Bounds) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn drag_between(
    robot: &cranpose::Robot,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    steps: usize,
    step_delay: Duration,
) {
    let _ = robot.mouse_move(start_x, start_y);
    std::thread::sleep(Duration::from_millis(50));
    let _ = robot.mouse_down();
    std::thread::sleep(Duration::from_millis(50));

    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let x = start_x + (end_x - start_x) * t;
        let y = start_y + (end_y - start_y) * t;
        let _ = robot.mouse_move(x, y);
        std::thread::sleep(step_delay);
    }

    let _ = robot.mouse_up();
}

pub fn click_button(robot: &cranpose::Robot, name: &str) -> bool {
    for _ in 0..60 {
        if let Ok(Some((x, y, w, h))) = robot.find_text_bounds(name) {
            let bounds = (x, y, w, h);
            let (center_x, center_y) = bounds_center(bounds);
            if !(0.0..=ROBOT_VIEWPORT_WIDTH).contains(&center_x) {
                let (start_x, end_x) = if center_x > ROBOT_VIEWPORT_WIDTH {
                    (ROBOT_VIEWPORT_WIDTH * 0.75, ROBOT_VIEWPORT_WIDTH * 0.25)
                } else {
                    (ROBOT_VIEWPORT_WIDTH * 0.25, ROBOT_VIEWPORT_WIDTH * 0.75)
                };
                let drag_y = center_y.clamp(24.0, 64.0);
                drag_between(
                    robot,
                    start_x,
                    drag_y,
                    end_x,
                    drag_y,
                    12,
                    Duration::from_millis(16),
                );
                std::thread::sleep(Duration::from_millis(120));
                continue;
            }
            let _ = robot.click(center_x, center_y);
            std::thread::sleep(Duration::from_millis(200));
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    println!("  ✗ Button '{name}' not found");
    false
}

pub fn click_first_visible_comments_button(robot: &cranpose::Robot, list_bounds: Bounds) -> bool {
    let Ok(elements) = robot.get_semantics() else {
        return false;
    };

    let (list_x, list_y, list_w, list_h) = list_bounds;
    let list_right = list_x + list_w;
    let list_bottom = list_y + list_h;
    let mut stories = Vec::new();
    for root in &elements {
        collect_story_elements(root, &mut stories);
    }

    stories.sort_by(|left, right| {
        left.bounds
            .y
            .partial_cmp(&right.bounds.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for story_elem in stories {
        let Some((x, y, w, h)) =
            find_button(story_elem, &format!("View {MOCK_COMMENT_COUNT} comments"))
        else {
            continue;
        };
        let center_x = x + w / 2.0;
        let center_y = y + h / 2.0;
        if center_x < list_x || center_x > list_right || center_y < list_y || center_y > list_bottom
        {
            continue;
        }
        println!(
            "  ✓ Clicking visible comments button in {}",
            story_elem.text.as_deref().unwrap_or("[unknown story]")
        );
        let _ = robot.click(center_x, center_y);
        std::thread::sleep(Duration::from_millis(200));
        return true;
    }

    println!("  ✗ No visible comments button found inside the list viewport");
    false
}

pub fn wait_for_text(robot: &cranpose::Robot, text: &str) -> bool {
    for _ in 0..60 {
        if find_in_semantics(robot, |elem| find_text_exact(elem, text)).is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

pub fn wait_for_no_text(robot: &cranpose::Robot, text: &str) -> bool {
    for _ in 0..60 {
        if find_in_semantics(robot, |elem| find_text_exact(elem, text)).is_none() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

pub fn wait_for_comments_data(robot: &cranpose::Robot) -> bool {
    wait_for_text(robot, INITIAL_COMMENTS_STATUS)
}

pub fn semantics_bounds(robot: &cranpose::Robot, label: &str) -> Option<Bounds> {
    let elements = robot.get_semantics().ok()?;
    find_element_by_text_exact(&elements, label).map(|elem| {
        (
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        )
    })
}

pub fn scroll_until_text_visible_in_bounds(
    robot: &cranpose::Robot,
    viewport: Bounds,
    text: &str,
    max_drags: usize,
) -> bool {
    for _ in 0..=max_drags {
        if semantics_bounds(robot, text).is_some_and(|bounds| bounds_intersect(bounds, viewport)) {
            return true;
        }

        let (x, y, w, h) = viewport;
        raw_drag(
            robot,
            x + w * 0.5,
            y + h * 0.78,
            y + h * 0.24,
            12,
            Duration::from_millis(16),
        );
        let _ = robot.wait_for_idle();
        std::thread::sleep(Duration::from_millis(80));
    }

    false
}

fn bounds_intersect(a: Bounds, b: Bounds) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    ax + aw > bx && ax < bx + bw && ay + ah > by && ay < by + bh
}

pub fn visible_comment_numbers(robot: &cranpose::Robot, list_bounds: Bounds) -> Vec<usize> {
    let Ok(elements) = robot.get_semantics() else {
        return Vec::new();
    };

    let mut numbers = Vec::new();
    for root in &elements {
        collect_visible_comment_numbers(root, list_bounds, &mut numbers);
    }

    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

pub fn scroll_until_comment_visible(
    robot: &cranpose::Robot,
    viewport: Bounds,
    max_drags: usize,
) -> Vec<usize> {
    for _ in 0..=max_drags {
        let numbers = visible_comment_numbers(robot, viewport);
        if !numbers.is_empty() {
            return numbers;
        }

        let (x, y, w, h) = viewport;
        raw_drag(
            robot,
            x + w * 0.5,
            y + h * 0.72,
            y + h * 0.38,
            10,
            Duration::from_millis(16),
        );
        let _ = robot.wait_for_idle();
        std::thread::sleep(Duration::from_millis(80));
    }

    Vec::new()
}

fn collect_visible_comment_numbers(
    elem: &SemanticElement,
    list_bounds: Bounds,
    numbers: &mut Vec<usize>,
) {
    if bounds_intersect(
        (
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ),
        list_bounds,
    ) {
        if let Some(number) = find_comment_number(elem) {
            numbers.push(number);
        }
    }

    for child in &elem.children {
        collect_visible_comment_numbers(child, list_bounds, numbers);
    }
}

pub fn visible_mock_story_numbers(robot: &cranpose::Robot, list_bounds: Bounds) -> Vec<usize> {
    let Ok(elements) = robot.get_semantics() else {
        return Vec::new();
    };

    let (list_x, list_y, list_w, list_h) = list_bounds;
    let list_right = list_x + list_w;
    let list_bottom = list_y + list_h;
    let mut numbers = Vec::new();
    let mut stories = Vec::new();
    for root in &elements {
        collect_story_elements(root, &mut stories);
    }

    for story in stories {
        let story_right = story.bounds.x + story.bounds.width;
        let story_bottom = story.bounds.y + story.bounds.height;
        let intersects_viewport = story_bottom > list_y
            && story.bounds.y < list_bottom
            && story_right > list_x
            && story.bounds.x < list_right;
        if !intersects_viewport {
            continue;
        }
        if let Some(number) = find_mock_story_number(story) {
            numbers.push(number);
        }
    }

    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

pub fn settle_visible_mock_story_numbers(
    robot: &cranpose::Robot,
    list_bounds: Bounds,
    max_attempts: usize,
    sample_delay: Duration,
) -> Vec<usize> {
    let mut previous = visible_mock_story_numbers(robot, list_bounds);
    let mut stable_samples = 0usize;

    for _ in 0..max_attempts {
        std::thread::sleep(sample_delay);
        let current = visible_mock_story_numbers(robot, list_bounds);
        if current == previous {
            stable_samples += 1;
            if stable_samples >= 2 {
                return current;
            }
        } else {
            stable_samples = 0;
            previous = current;
        }
    }

    previous
}

pub fn raw_drag(
    robot: &cranpose::Robot,
    x: f32,
    start_y: f32,
    end_y: f32,
    steps: usize,
    step_delay: Duration,
) {
    drag_between(robot, x, start_y, x, end_y, steps, step_delay);
}

pub fn fail(robot: &cranpose::Robot, message: impl Into<String>) -> ! {
    let message = message.into();
    println!("  ✗ FAIL: {message}");
    if let Ok(elements) = robot.get_semantics() {
        print_semantics_with_bounds(&elements, 0);
    }
    std::process::exit(1);
}
