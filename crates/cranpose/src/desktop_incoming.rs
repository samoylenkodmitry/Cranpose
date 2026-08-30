use std::path::Path;

use cranpose_services::{IncomingContent, media::uri_for_path, publish_incoming_content};

pub(crate) fn publish_file(path: &Path) {
    let mut content = IncomingContent::from_uri(uri_for_path(path));
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        content = content.with_name(name);
    }
    publish_incoming_content(content);
}

pub(crate) fn publish_launch_documents() {
    for path in launch_documents(std::env::args().skip(1)) {
        publish_file(&path);
    }
}

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
