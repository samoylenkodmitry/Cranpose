use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use cranpose_services::{
    IncomingContent, media::uri_for_path, publish_incoming_content, publish_opened_link,
};
use winit::{
    data_transfer::{DataTransferId, TypeHint, TypedData},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, DndAction},
};

#[derive(Default)]
pub(crate) struct FileDrops {
    waiting: Vec<Arc<dyn TypedData>>,
}

impl FileDrops {
    pub(crate) fn handle(&mut self, event_loop: &dyn ActiveEventLoop, event: &WindowEvent) {
        match event {
            WindowEvent::DragEntered { id, .. } => Self::offer(event_loop, *id),
            WindowEvent::DragDropped { id, .. } => Self::request(event_loop, *id),
            WindowEvent::DataTransferReceived { value, .. } => self.receive(Arc::clone(value)),
            _ => {}
        }
    }

    fn offer(event_loop: &dyn ActiveEventLoop, id: DataTransferId) {
        let carries_files = event_loop
            .data_transfer(id)
            .is_ok_and(|transfer| transfer.has_type(&TypeHint::UriList));
        if !carries_files {
            return;
        }
        if let Err(error) = event_loop.set_valid_dnd_actions(id, &[DndAction::Copy]) {
            log::debug!("file drag could not be accepted: {error}");
        }
    }

    fn request(event_loop: &dyn ActiveEventLoop, id: DataTransferId) {
        if let Err(error) = event_loop.fetch_data_transfer(id, &TypeHint::UriList) {
            log::debug!("dropped files could not be requested: {error}");
        }
    }

    fn receive(&mut self, value: Arc<dyn TypedData>) {
        self.waiting.push(value);
        self.waiting.retain(|data| match data.try_as_file_paths() {
            Ok(paths) => {
                paths.iter().for_each(|path| publish_file(path));
                false
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => true,
            Err(error) => {
                log::debug!("dropped data is not a file list: {error}");
                false
            }
        });
    }
}

pub(crate) fn publish_file(path: &Path) {
    let mut content = IncomingContent::from_uri(uri_for_path(path));
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        content = content.with_name(name);
    }
    publish_incoming_content(content);
}

/// Publishes the documents and the link that the command line names: what
/// Windows and Linux pass to an application that opens a file or a URL in one
/// of its schemes.
pub(crate) fn publish_launch_arguments() {
    for item in launch_items(std::env::args().skip(1)) {
        match item {
            LaunchItem::Document(path) => publish_file(&path),
            LaunchItem::Link(link) => publish_opened_link(link),
        }
    }
}

#[derive(Debug, PartialEq)]
enum LaunchItem {
    Document(PathBuf),
    Link(String),
}

fn launch_items<I>(arguments: I) -> Vec<LaunchItem>
where
    I: IntoIterator<Item = String>,
{
    arguments
        .into_iter()
        .filter(|argument| !argument.starts_with('-'))
        .filter_map(|argument| {
            let path = PathBuf::from(&argument);
            if path.is_file() {
                Some(LaunchItem::Document(path))
            } else {
                is_link(&argument).then_some(LaunchItem::Link(argument))
            }
        })
        .collect()
}

/// Whether `argument` is a URL in a scheme, `myapp://item/7` or `myapp:item`.
/// A scheme takes two characters at least, so `C:\file` is no link.
fn is_link(argument: &str) -> bool {
    let Some((scheme, _)) = argument.split_once(':') else {
        return false;
    };
    scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
}

#[cfg(test)]
#[path = "tests/desktop_incoming_tests.rs"]
mod tests;
