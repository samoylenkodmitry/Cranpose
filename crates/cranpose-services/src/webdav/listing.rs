use std::collections::BTreeSet;

use crate::{ContentError, percent_decode_lossy};

pub(super) enum EntryKind {
    Folder,
    File {
        len: Option<u64>,
        mime_type: Option<String>,
    },
}

pub(super) struct Entry {
    pub(super) url: url::Url,
    pub(super) kind: EntryKind,
}

pub(super) fn parse(bytes: &[u8], parent: &url::Url) -> Result<Vec<Entry>, ContentError> {
    let text = std::str::from_utf8(bytes).map_err(|e| ContentError::Io(e.to_string()))?;
    let document = roxmltree::Document::parse(text).map_err(|e| ContentError::Io(e.to_string()))?;
    let root = document.root_element();
    if !root.has_tag_name(("DAV:", "multistatus")) {
        return Err(ContentError::Io(
            "The address did not return a WebDAV directory".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for response in root
        .children()
        .filter(|n| n.has_tag_name(("DAV:", "response")))
    {
        if let Some(entry) = parse_entry(response, parent)?
            && seen.insert(entry.url.to_string())
        {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn parse_entry(
    response: roxmltree::Node<'_, '_>,
    parent: &url::Url,
) -> Result<Option<Entry>, ContentError> {
    let Some(href) = child_text(response, "href") else {
        return Ok(None);
    };
    let Some(mut url) = child_url(parent, href)? else {
        return Ok(None);
    };
    let properties: Vec<_> = response
        .children()
        .filter(|n| n.has_tag_name(("DAV:", "propstat")))
        .filter(|n| {
            child_text(*n, "status").is_some_and(|s| s.split_whitespace().nth(1) == Some("200"))
        })
        .flat_map(|n| n.children().filter(|p| p.has_tag_name(("DAV:", "prop"))))
        .collect();
    if properties.is_empty() {
        return Ok(None);
    }
    let collection = properties
        .iter()
        .flat_map(roxmltree::Node::children)
        .filter(|n| n.has_tag_name(("DAV:", "resourcetype")))
        .any(|n| n.children().any(|c| c.has_tag_name(("DAV:", "collection"))));
    let kind = if collection {
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        EntryKind::Folder
    } else {
        EntryKind::File {
            len: properties
                .iter()
                .find_map(|p| child_text(*p, "getcontentlength"))
                .and_then(|s| s.parse().ok()),
            mime_type: properties
                .iter()
                .find_map(|p| child_text(*p, "getcontenttype"))
                .map(str::to_owned),
        }
    };
    Ok(Some(Entry { url, kind }))
}

fn child_url(parent: &url::Url, href: &str) -> Result<Option<url::Url>, ContentError> {
    let mut url = parent
        .join(href)
        .map_err(|e| ContentError::Io(e.to_string()))?;
    url.set_fragment(None);
    if url.origin() != parent.origin() || !url.username().is_empty() || url.password().is_some() {
        return Err(ContentError::PermissionDenied(
            "WebDAV entry leaves the selected server".into(),
        ));
    }
    let parent_path = percent_decode_lossy(parent.path());
    let child_path = percent_decode_lossy(url.path());
    if child_path.trim_end_matches('/') == parent_path.trim_end_matches('/') {
        return Ok(None);
    }
    let child = child_path
        .strip_prefix(parent_path.as_str())
        .ok_or_else(|| {
            ContentError::PermissionDenied("WebDAV entry leaves the selected folder".into())
        })?
        .trim_end_matches('/');
    if child.is_empty() || child.contains(['/', '\\']) || child == "." || child == ".." {
        return Err(ContentError::PermissionDenied(
            "WebDAV entry is not an immediate child".into(),
        ));
    }
    Ok(Some(url))
}

fn child_text<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|n| n.has_tag_name(("DAV:", name)))?
        .text()
}
