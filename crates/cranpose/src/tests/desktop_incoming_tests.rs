use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use cranpose_services::{IncomingContentObserver, observe_incoming_content};
use winit::data_transfer::TransferType;

use super::*;

#[derive(Debug)]
struct StagedFileList {
    paths: Vec<PathBuf>,
    reads_before_ready: AtomicUsize,
}

impl StagedFileList {
    fn ready_after(reads_before_ready: usize, names: &[&str]) -> Arc<dyn TypedData> {
        Arc::new(Self {
            paths: names
                .iter()
                .map(|name| PathBuf::from("/drops").join(name))
                .collect(),
            reads_before_ready: AtomicUsize::new(reads_before_ready),
        })
    }
}

impl TypedData for StagedFileList {
    fn type_(&self) -> &dyn TransferType {
        &TypeHint::UriList
    }

    fn try_read(&self) -> Option<Box<dyn io::BufRead>> {
        None
    }

    fn try_as_uris(&self) -> io::Result<Vec<String>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn try_as_file_paths(&self) -> io::Result<Vec<PathBuf>> {
        let still_blocked = self
            .reads_before_ready
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        if still_blocked {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        Ok(self.paths.clone())
    }

    fn try_as_string(&self) -> io::Result<String> {
        Err(io::ErrorKind::InvalidData.into())
    }
}

#[derive(Debug)]
struct PlainText;

impl TypedData for PlainText {
    fn type_(&self) -> &dyn TransferType {
        &TypeHint::Plaintext
    }

    fn try_read(&self) -> Option<Box<dyn io::BufRead>> {
        None
    }

    fn try_as_uris(&self) -> io::Result<Vec<String>> {
        Err(io::ErrorKind::InvalidData.into())
    }

    fn try_as_file_paths(&self) -> io::Result<Vec<PathBuf>> {
        Err(io::ErrorKind::InvalidData.into())
    }

    fn try_as_string(&self) -> io::Result<String> {
        Ok("plain".to_owned())
    }
}

fn published_names_matching(
    prefix: &'static str,
) -> (Arc<Mutex<Vec<String>>>, IncomingContentObserver) {
    let names = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&names);
    let observer = observe_incoming_content(move |item| {
        let name = item.display_name();
        if name.starts_with(prefix)
            && let Ok(mut names) = sink.lock()
        {
            names.push(name);
        }
    });
    (names, observer)
}

fn snapshot(names: &Mutex<Vec<String>>) -> Vec<String> {
    names.lock().map(|names| names.clone()).unwrap_or_default()
}

#[test]
fn a_dropped_file_list_publishes_every_file() {
    let (names, _observer) = published_names_matching("drop-every-");
    let mut drops = FileDrops::default();

    drops.receive(StagedFileList::ready_after(
        0,
        &["drop-every-a.txt", "drop-every-b.png"],
    ));

    assert_eq!(snapshot(&names), ["drop-every-a.txt", "drop-every-b.png"]);
    assert!(drops.waiting.is_empty());
}

#[test]
fn a_file_list_that_is_not_ready_is_read_again_on_the_next_transfer() {
    let (names, _observer) = published_names_matching("drop-later-");
    let mut drops = FileDrops::default();

    drops.receive(StagedFileList::ready_after(1, &["drop-later-a.txt"]));
    assert!(snapshot(&names).is_empty());
    assert_eq!(drops.waiting.len(), 1);

    drops.receive(StagedFileList::ready_after(0, &["drop-later-b.txt"]));
    assert_eq!(snapshot(&names), ["drop-later-a.txt", "drop-later-b.txt"]);
    assert!(drops.waiting.is_empty());
}

#[test]
fn data_that_is_not_a_file_list_is_discarded() {
    let mut drops = FileDrops::default();

    drops.receive(Arc::new(PlainText));

    assert!(drops.waiting.is_empty());
}

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
