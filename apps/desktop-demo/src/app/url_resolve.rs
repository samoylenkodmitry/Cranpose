const RAW_GITHUB_HOST: &str = "raw.githubusercontent.com";

fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
    !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

fn split_base(base: &str) -> Option<(&str, &str, &str)> {
    let (scheme, rest) = base.split_once("://")?;
    let (host, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if scheme.is_empty() || host.is_empty() {
        return None;
    }
    Some((scheme, host, path))
}

fn site_root(scheme: &str, host: &str, path: &str) -> String {
    let origin = format!("{scheme}://{host}");
    if !host.eq_ignore_ascii_case(RAW_GITHUB_HOST) {
        return origin;
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let kept = if segments.len() > 5 && segments[2] == "refs" {
        5
    } else if segments.len() > 3 {
        3
    } else {
        return origin;
    };
    format!("{origin}/{}", segments[..kept].join("/"))
}

fn normalize_path(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    let mut normalized = format!("/{}", out.join("/"));
    let is_directory =
        path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..") || path == "..";
    if is_directory && !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

/// Resolves a markdown link or image target against the document's own URL.
///
/// A root-relative target is resolved against the site root, which on
/// `raw.githubusercontent.com` is the repository ref — `/owner/repo/<ref>` or
/// `/owner/repo/refs/heads/<branch>` — because that host serves a repository
/// rather than a site, and a document's `/assets/x.png` means the file at the
/// repository root, not at the host root.
pub(crate) fn resolve_url(base: &str, target: &str) -> String {
    let target = target.trim();
    if target.is_empty() || has_scheme(target) {
        return target.to_string();
    }
    let Some((scheme, host, path)) = split_base(base.trim()) else {
        return target.to_string();
    };
    if let Some(rest) = target.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    if target.starts_with('/') {
        return format!(
            "{}{}",
            site_root(scheme, host, path),
            normalize_path(target)
        );
    }
    let directory = path.rsplit_once('/').map(|(head, _)| head).unwrap_or("");
    format!(
        "{scheme}://{host}{}",
        normalize_path(&format!("{directory}/{target}"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str =
        "https://raw.githubusercontent.com/owner/repo/refs/heads/master/notes/post.md";

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
}
