use super::*;

const DOC: &str = "https://raw.githubusercontent.com/owner/repo/refs/heads/master/notes/post.md";

#[test]
fn an_absolute_target_is_left_alone() {
    assert_eq!(
        resolve_url(DOC, "https://example.com/a.png"),
        "https://example.com/a.png"
    );
    assert_eq!(
        resolve_url(DOC, "data:image/png;base64,AAAA"),
        "data:image/png;base64,AAAA"
    );
}

#[test]
fn a_root_relative_target_resolves_against_the_repository_ref() {
    assert_eq!(
        resolve_url(DOC, "/assets/cat.png"),
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/assets/cat.png"
    );
}

#[test]
fn a_short_form_raw_url_keeps_owner_repo_and_ref() {
    assert_eq!(
        resolve_url(
            "https://raw.githubusercontent.com/owner/repo/master/notes/post.md",
            "/assets/cat.png"
        ),
        "https://raw.githubusercontent.com/owner/repo/master/assets/cat.png"
    );
}

#[test]
fn a_root_relative_target_resolves_against_the_host_elsewhere() {
    assert_eq!(
        resolve_url("https://example.com/docs/post.md", "/assets/cat.png"),
        "https://example.com/assets/cat.png"
    );
}

#[test]
fn a_sibling_target_resolves_against_the_document_directory() {
    assert_eq!(
        resolve_url(DOC, "cat.png"),
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/notes/cat.png"
    );
    assert_eq!(
        resolve_url(DOC, "./cat.png"),
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/notes/cat.png"
    );
}

#[test]
fn a_parent_target_climbs_one_directory() {
    assert_eq!(
        resolve_url(DOC, "../img/cat.png"),
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/img/cat.png"
    );
}

#[test]
fn a_protocol_relative_target_takes_the_base_scheme() {
    assert_eq!(
        resolve_url(DOC, "//cdn.example.com/cat.png"),
        "https://cdn.example.com/cat.png"
    );
}

#[test]
fn an_unusable_base_leaves_the_target_untouched() {
    assert_eq!(resolve_url("", "/assets/cat.png"), "/assets/cat.png");
    assert_eq!(resolve_url("not a url", "cat.png"), "cat.png");
}

#[test]
fn a_directory_target_keeps_its_trailing_slash() {
    assert_eq!(
        resolve_url(DOC, "/leetcode/"),
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/leetcode/"
    );
}

#[test]
fn an_empty_target_stays_empty() {
    assert_eq!(resolve_url(DOC, ""), "");
    assert_eq!(resolve_url(DOC, "   "), "");
}
