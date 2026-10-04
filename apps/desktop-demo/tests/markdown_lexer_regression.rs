use std::{sync::Arc, thread, time::Duration};

use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClientRef, StubHttpClient};
use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app;

fn markdown_robot(markdown: String) -> RobotTestRule<TestRenderer> {
    let client: HttpClientRef = Arc::new(StubHttpClient::with_body(markdown.into_bytes()));
    let local = local_http_client();
    RobotTestRule::new(800, 600, TestRenderer::default(), move || {
        let client = client.clone();
        CompositionLocalProvider([local.provides(client)], || {
            app::MarkdownViewerRobotApp();
        });
    })
}

fn fetch_and_assert_source(language: &str, code: &str) {
    let markdown = format!("```{language}\n{code}\n```");
    let mut robot = markdown_robot(markdown);
    assert!(
        robot.find_by_text("Fetch").click(),
        "Fetch button is present"
    );

    for _ in 0..100 {
        let visible_text = robot.get_all_text();
        assert!(
            !visible_text.iter().any(|text| text.contains("Error:")),
            "stub Markdown request should succeed: {visible_text:?}"
        );
        if visible_text.iter().any(|text| text.contains(code)) {
            return;
        }
        robot.advance_time(16_666_667);
        thread::sleep(Duration::from_millis(1));
    }

    panic!("fetched {language} code was not preserved exactly: {code:?}");
}

#[test]
fn fetched_rust_c_and_python_code_preserve_incomplete_prefixes_and_unicode() {
    for (language, code) in [
        ("rust", "let value = 0x"),
        ("rust", "let crab = 0x🦀;"),
        ("c", "int value = 0b"),
        ("c", "int crab = 0b🦀;"),
        ("python", "value = 0x"),
        ("python", "crab = 0x🦀"),
    ] {
        fetch_and_assert_source(language, code);
    }
}
