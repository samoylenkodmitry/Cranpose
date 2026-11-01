use std::path::PathBuf;

pub(crate) fn bare_path(uri: &str) -> Option<PathBuf> {
    if let Some((scheme, _)) = uri.split_once(':') {
        let mut bytes = scheme.bytes();
        // A single letter remains a Windows drive, including drive-relative paths.
        if scheme.len() > 1
            && bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        {
            return None;
        }
    }
    if uri.is_empty() {
        None
    } else {
        Some(PathBuf::from(uri))
    }
}
