//! Files a desktop hands the application without a picker.
//!
//! Two ways in, and both arrive as the same [`IncomingContent`] a share or a
//! picked file does, so an application collects one stream rather than growing
//! a desktop-shaped path of its own:
//!
//! * dropped on the window, which winit reports as `DragDropped`;
//! * named on the command line, which is what a desktop shell passes when a
//!   document is opened with this application.
//!
//! The command-line half covers Linux and Windows, where opening a document
//! launches the program with its path in `argv`. A macOS `.app` bundle is told
//! through an Apple Event instead, which winit does not surface — so a Mac
//! delivers dropped files and not opened ones, and this says so rather than
//! implying coverage it does not have.

use std::path::Path;

use cranpose_services::{media::uri_for_path, publish_incoming_content, IncomingContent};

/// Publishes one file as incoming content.
///
/// The URI form is what the rest of the framework already understands: a
/// `file:` URI resolves through the content resolver, so a dropped file, a
/// picked file and a shared file reach the application as the same handle.
pub(crate) fn publish_file(path: &Path) {
    let mut content = IncomingContent::from_uri(uri_for_path(path));
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        content = content.with_name(name);
    }
    publish_incoming_content(content);
}

/// Publishes the documents this process was launched to open.
///
/// Called once as the shell starts, before the first composition, so the
/// backlog is waiting for the first collector rather than arriving before
/// anything can observe it.
pub(crate) fn publish_launch_documents() {
    for path in launch_documents(std::env::args().skip(1)) {
        publish_file(&path);
    }
}

/// The arguments that name an existing file.
///
/// An option and its value are not documents, and neither is a path to
/// something that is not there: a desktop application is launched with all
/// three, and only the files are content.
fn launch_documents<I>(arguments: I) -> Vec<std::path::PathBuf>
where
    I: IntoIterator<Item = String>,
{
    arguments
        .into_iter()
        .filter(|argument| !argument.starts_with('-'))
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_option_is_not_a_document() {
        let arguments = ["--headless".to_owned(), "-v".to_owned()];
        assert!(launch_documents(arguments).is_empty());
    }

    #[test]
    fn a_path_to_nothing_is_not_a_document() {
        let arguments = ["/cranpose/no/such/file.txt".to_owned()];
        assert!(launch_documents(arguments).is_empty());
    }

    #[test]
    fn an_existing_file_named_on_the_command_line_is_a_document() {
        let directory = crate::test_scratch_dir("launch-document");
        let path = directory.join("cranpose-launch-document.txt");
        std::fs::write(&path, b"opened").expect("write");
        let found = launch_documents([path.to_string_lossy().into_owned()]);
        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(found, vec![path]);
    }

    #[test]
    fn a_directory_is_not_a_document() {
        let directory = crate::test_scratch_dir("launch-directory");
        let arguments = [directory.to_string_lossy().into_owned()];
        let found = launch_documents(arguments);
        let _ = std::fs::remove_dir_all(&directory);
        assert!(found.is_empty());
    }
}
