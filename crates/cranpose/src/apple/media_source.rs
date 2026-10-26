#![expect(unsafe_code, reason = "Foundation security-scoped resource access")]

use std::{fs::File, io, path::Path, sync::Arc};

use cranpose_services::{MediaSourceHandle, MediaSourceOpener, set_platform_media_source_opener};
use objc2::{Message, rc::Retained, runtime::Bool};
use objc2_foundation::{
    NSData, NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
};

const PREFIX: &str = "apple-bookmark:";

pub(crate) fn register() {
    set_platform_media_source_opener(Arc::new(BookmarkOpener));
}

pub(crate) fn reference(scope: &NSURL, path: &Path) -> io::Result<String> {
    let scope_path = scope
        .path()
        .ok_or_else(|| io::Error::other("selected resource has no path"))?;
    let scope_path = scope_path.to_string();
    let relative = path
        .strip_prefix(&scope_path)
        .map_err(|_| io::Error::other("audio file is outside the selected resource"))?;
    let _access = Access::new(scope.retain());
    #[cfg(target_os = "macos")]
    let options = NSURLBookmarkCreationOptions::WithSecurityScope
        | NSURLBookmarkCreationOptions::SecurityScopeAllowOnlyReadAccess;
    #[cfg(not(target_os = "macos"))]
    let options = NSURLBookmarkCreationOptions::empty();
    let bookmark = scope
        .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            options, None, None,
        )
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(format!(
        "{PREFIX}{}/{}",
        hex(&bookmark.to_vec()),
        hex(relative.to_string_lossy().as_bytes())
    ))
}

struct Access {
    url: Retained<NSURL>,
    active: bool,
}

impl Access {
    fn new(url: Retained<NSURL>) -> Self {
        // The retained URL stays alive until its matching stop call.
        let active = unsafe { url.startAccessingSecurityScopedResource() };
        Self { url, active }
    }
}

impl Drop for Access {
    fn drop(&mut self) {
        if self.active {
            // This guard balances the successful start call on the same URL.
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }
}

struct BookmarkOpener;

impl MediaSourceOpener for BookmarkOpener {
    fn reference(&self, path: &Path) -> io::Result<String> {
        let path_text = NSString::from_str(&path.to_string_lossy());
        let url = NSURL::fileURLWithPath(&path_text);
        reference(&url, path)
    }

    fn open(&self, uri: &str) -> io::Result<MediaSourceHandle> {
        let (bookmark, relative) = uri
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.split_once('/'))
            .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "unknown media reference"))?;
        let bytes = unhex(bookmark)?;
        let relative = String::from_utf8(unhex(relative)?)
            .map_err(|_| io::Error::other("invalid bookmark path"))?;
        if Path::new(&relative)
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(io::Error::other("invalid bookmark path"));
        }
        let data = NSData::with_bytes(&bytes);
        let mut stale = Bool::NO;
        #[cfg(target_os = "macos")]
        let options = NSURLBookmarkResolutionOptions::WithSecurityScope;
        #[cfg(not(target_os = "macos"))]
        let options = NSURLBookmarkResolutionOptions::empty();
        // Data and the stale-result pointer remain alive throughout Foundation's call.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data, options, None, &mut stale,
            )
        }
        .map_err(|error| io::Error::other(error.to_string()))?;
        let access = Access::new(url);
        let root = access
            .url
            .path()
            .ok_or_else(|| io::Error::other("bookmark has no file path"))?;
        let path = Path::new(&root.to_string()).join(relative);
        let file = File::open(path)?;
        let len = file.metadata().ok().map(|metadata| metadata.len());
        Ok(MediaSourceHandle::new(file, len).with_access(access))
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

fn unhex(text: &str) -> io::Result<Vec<u8>> {
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return Err(io::Error::other("invalid media bookmark"));
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .map_err(|_| io::Error::other("invalid media bookmark"))
        })
        .collect()
}
