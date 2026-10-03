use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use cranpose_ui::{Box, BoxSpec, Modifier, Size};

use super::*;

#[composable]
fn MarkdownDocument(markdown: &'static str, base_url: &'static str, width: f32) {
    let blocks = cranpose_core::rememberKeyed((markdown, base_url), |(markdown, base_url)| {
        Rc::<[MarkdownBlock]>::from(split_large_markdown_blocks(markdown_to_blocks(
            markdown, base_url,
        )))
    });
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(18.0)),
        move || {
            for (index, block) in blocks.iter().enumerate() {
                cranpose_core::with_key(&index, || {
                    render_markdown_block(block, MarkdownAppearance::Reader, width);
                });
            }
        },
    );
}

fn example(id: &str) -> &'static str {
    let guide = include_str!("../../../../../docs/guide.md");
    let marker = format!("```rust preview={id}");
    let start = guide.find(&marker).expect("guide example");
    let body = &guide[start..];
    let end = body[3..].find("\n```").expect("example end") + 7;
    &body[..end]
}

#[test]
fn counter_example_uses_the_displayed_markdown_source() {
    let mut robot = RobotTestRule::new(840, 2400, TestRenderer::default(), || {
        let viewport = cranpose_core::rememberMutableStateOf(Size::default);
        Box(
            Modifier::empty()
                .fill_max_size()
                .report_size_state(viewport),
            BoxSpec::default(),
            move || {
                if viewport.get().width > 0.0 {
                    MarkdownDocument(example("counter"), "", viewport.get().width);
                }
            },
        );
    });
    assert!(robot.find_by_text("Count: 0").exists());
    let button = robot
        .get_all_rects()
        .into_iter()
        .find(|(_, text)| text.as_deref() == Some("Increment"))
        .expect("preview button")
        .0;
    assert!(robot.click_at(
        button.x + button.width * 0.5,
        button.y + button.height * 0.5
    ));
    assert!(robot.find_by_text("Count: 1").exists());
    for width in [390, 840] {
        robot.set_viewport(width, 2400);
        robot.wait_for_idle();
        assert!(
            robot.find_by_text("Count: 1").exists(),
            "preview state survives width {width}"
        );
    }
}

#[test]
fn previews_move_below_code_on_a_phone() {
    for width in [390, 840] {
        let mut robot = RobotTestRule::new(width, 2400, TestRenderer::default(), move || {
            MarkdownDocument(example("scaled_text"), "", width as f32);
        });
        robot.wait_for_idle();
        let rects = robot.get_all_rects();
        let code = rects
            .iter()
            .find(|(_, text)| {
                text.as_deref()
                    .is_some_and(|text| text.starts_with("use cranpose::text::TextUnit;"))
            })
            .expect("visible source")
            .0;
        let preview = rects
            .iter()
            .find(|(_, text)| text.as_deref() == Some("Your library"))
            .expect("rendered label")
            .0;
        if width == 390 {
            assert!(
                preview.y >= code.y + code.height,
                "preview below source: {code:?}, {preview:?}"
            );
        } else {
            assert!(
                preview.x >= code.x + code.width,
                "preview beside source: {code:?}, {preview:?}"
            );
        }
    }
}

#[test]
fn glass_button_preview_updates_its_label() {
    let mut robot = RobotTestRule::new(390, 1600, TestRenderer::default(), || {
        cranpose::liquid::LiquidTheme(cranpose::liquid::LiquidThemeSpec::default(), || {
            MarkdownDocument(example("glass_button"), "", 390.0);
        });
    });
    let button = robot
        .get_all_rects()
        .into_iter()
        .find(|(_, text)| text.as_deref() == Some("Save"))
        .expect("glass button label")
        .0;
    assert!(robot.click_at(
        button.x + button.width * 0.5,
        button.y + button.height * 0.5
    ));
    assert!(robot.find_by_text("Saved").exists());
}

#[test]
fn markdown_table_keeps_columns_and_scrolls_horizontally() {
    let mut robot = RobotTestRule::new(320, 500, TestRenderer::default(), || {
        MarkdownDocument(
            "| Scope | API | Example |\n| :--- | :---: | ---: |\n| **Layout** | `Rule` | Label |\n| Input | Robot | Click |",
            "", 320.0,
        );
    });
    let before = robot.find_by_text("Example").bounds().expect("last column");
    robot.move_to(220.0, 65.0);
    robot.shell_mut().pointer_scrolled(-180.0, 0.0);
    robot.wait_for_idle();
    let after = robot
        .find_by_text("Example")
        .bounds()
        .expect("last column after pan");
    assert!(
        after.x < before.x - 80.0,
        "table pans: {before:?} -> {after:?}"
    );
    assert!(after.x + after.width <= 320.0);
    assert!(robot.find_by_text("Click").exists());
}
