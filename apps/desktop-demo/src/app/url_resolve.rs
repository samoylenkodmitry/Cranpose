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
    let directory = path.rsplit_once('/').map_or("", |(head, _)| head);
    format!(
        "{scheme}://{host}{}",
        normalize_path(&format!("{directory}/{target}"))
    )
}

#[cfg(test)]
#[path = "tests/url_resolve_tests.rs"]
mod tests;
