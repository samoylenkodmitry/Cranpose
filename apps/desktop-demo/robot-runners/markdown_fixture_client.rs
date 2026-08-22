use cranpose_services::{
    BytesBody, HttpClient, HttpControl, HttpFuture, HttpRequest, HttpResponse,
};
use std::path::PathBuf;

pub(crate) struct MarkdownFixtureClient {
    body: String,
}

impl MarkdownFixtureClient {
    #[allow(dead_code)]
    pub(crate) fn from_body(body: String) -> Self {
        Self { body }
    }

    pub(crate) fn default_leetcode_daily() -> Self {
        let path = default_leetcode_daily_fixture_path();
        let body = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!(
                "failed to read default Markdown robot fixture {}: {err}",
                path.display()
            )
        });
        Self { body }
    }
}

impl HttpClient for MarkdownFixtureClient {
    fn send<'a>(
        &'a self,
        request: &'a HttpRequest,
        _control: HttpControl,
    ) -> HttpFuture<'a, HttpResponse> {
        Box::pin(async move {
            Ok(HttpResponse::new(
                request.url.clone(),
                200,
                std::sync::Arc::new(BytesBody::new(self.body.clone())),
            ))
        })
    }
}

fn default_leetcode_daily_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("robot-fixtures")
        .join("leetcode_daily_default.md")
}
