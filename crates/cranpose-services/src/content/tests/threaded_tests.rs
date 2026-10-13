use std::{
    future::Future,
    pin::pin,
    sync::mpsc::{Receiver, channel},
    task::{Context, Waker},
};

use super::*;
use crate::{
    content::{FileSink, file_content},
    file_picker::{
        FilePicker, FilePickerError, FilePickerOptions, PickerFuture, SaveDocumentRequest,
    },
};

/// A source whose bytes come only when the test sends them.
struct Held {
    bytes: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for Held {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.bytes.recv() {
                Ok(bytes) => self.pending = bytes,
                Err(_) => return Ok(0),
            }
        }
        let taken = buffer.len().min(self.pending.len());
        buffer[..taken].copy_from_slice(&self.pending[..taken]);
        self.pending.drain(..taken);
        Ok(taken)
    }
}

#[test]
fn a_reader_leaves_the_awaiting_thread_free_while_its_source_waits() {
    let (send, bytes) = channel();
    let reader = pollster::block_on(open_threaded(
        move || {
            Ok(Held {
                bytes,
                pending: Vec::new(),
            })
        },
        |error| ContentError::Io(error.to_string()),
    ))
    .unwrap();
    let mut next = pin!(reader.read_chunk());
    let mut context = Context::from_waker(Waker::noop());
    assert!(next.as_mut().poll(&mut context).is_pending());
    send.send(b"late".to_vec()).unwrap();
    drop(send);
    assert_eq!(
        pollster::block_on(next).unwrap().as_deref(),
        Some(&b"late"[..])
    );
    assert_eq!(pollster::block_on(reader.read_chunk()).unwrap(), None);
}

#[test]
fn a_failed_open_is_the_open_error() {
    let opened = pollster::block_on(open_threaded::<std::fs::File, _, _>(
        || Err(ContentError::NotFound("absent".into())),
        |error| ContentError::Io(error.to_string()),
    ));
    assert!(matches!(opened, Err(ContentError::NotFound(_))));
}

/// A chooser that always picks `destination`, or nothing.
struct Chooses {
    destination: Option<std::path::PathBuf>,
}

impl FilePicker for Chooses {
    fn pick_file(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<crate::content::ContentHandle>, FilePickerError>> {
        Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
    }

    fn pick_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<crate::content::ContentFolderRef>, FilePickerError>> {
        Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
    }

    fn save_document(
        &self,
        _request: SaveDocumentRequest,
    ) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
        let destination = self.destination.clone();
        Box::pin(async move {
            Ok(match destination {
                Some(destination) => Some(FileSink::create(destination)?.handle()),
                None => None,
            })
        })
    }
}

#[test]
fn a_file_exports_into_the_chosen_place_and_a_cancel_exports_nothing() {
    let root = crate::test_scratch_dir("content-export");
    let source = root.join("model.bin");
    let payload: Vec<u8> = (0..DEFAULT_CHUNK_LEN * 2 + 7)
        .map(|index| index as u8)
        .collect();
    std::fs::write(&source, &payload).unwrap();
    let destination = root.join("chosen/model copy.bin");
    let request = || SaveDocumentRequest::new("model copy.bin", "application/octet-stream");

    let chosen = Chooses {
        destination: Some(destination.clone()),
    };
    assert_eq!(
        pollster::block_on(chosen.export_file(source.clone(), request())),
        Ok(true)
    );
    assert_eq!(std::fs::read(&destination).unwrap(), payload);

    let cancelled = Chooses { destination: None };
    assert_eq!(
        pollster::block_on(cancelled.export_file(source, request())),
        Ok(false)
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_copy_reports_the_bytes_so_far_after_each_chunk() {
    let root = crate::test_scratch_dir("content-copy");
    let source = root.join("in.bin");
    std::fs::write(&source, vec![1u8; DEFAULT_CHUNK_LEN + 3]).unwrap();
    let sink = FileSink::create(root.join("out.bin")).unwrap().handle();
    let mut seen = Vec::new();
    let copied = pollster::block_on(copy_content(&*file_content(&source), &sink, |bytes| {
        seen.push(bytes);
    }))
    .unwrap();
    assert_eq!(copied, DEFAULT_CHUNK_LEN as u64 + 3);
    assert_eq!(
        seen,
        vec![DEFAULT_CHUNK_LEN as u64, DEFAULT_CHUNK_LEN as u64 + 3]
    );
    assert_eq!(
        std::fs::read(root.join("out.bin")).unwrap().len(),
        DEFAULT_CHUNK_LEN + 3
    );
    std::fs::remove_dir_all(&root).unwrap();
}
