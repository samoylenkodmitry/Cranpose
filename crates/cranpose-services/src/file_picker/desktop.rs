use super::{FilePickerError, FilePickerOptions, PickerFuture, SaveDocumentRequest};
use crate::content::{
    ContentFolderRef, ContentHandle, ContentSinkRef, FileSink, file_content, file_folder,
};

fn dialog(options: &FilePickerOptions) -> rfd::AsyncFileDialog {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(title) = &options.title {
        dialog = dialog.set_title(title);
    }
    for filter in &options.filters {
        let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(filter.label.clone(), &extensions);
    }
    dialog
}

pub(super) fn pick_file(
    options: FilePickerOptions,
) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
    Box::pin(async move {
        Ok(dialog(&options)
            .pick_file()
            .await
            .map(|handle| file_content(handle.path())))
    })
}

pub(super) fn pick_files(
    options: FilePickerOptions,
) -> PickerFuture<Result<Vec<ContentHandle>, FilePickerError>> {
    Box::pin(async move {
        Ok(dialog(&options)
            .pick_files()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|handle| file_content(handle.path()))
            .collect())
    })
}

pub(super) fn pick_folder(
    options: FilePickerOptions,
) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
    Box::pin(async move {
        Ok(dialog(&options)
            .pick_folder()
            .await
            .map(|handle| file_folder(handle.path())))
    })
}

pub(super) fn save_document(
    request: SaveDocumentRequest,
) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
    Box::pin(async move {
        let mut dialog = rfd::AsyncFileDialog::new().set_file_name(&request.file_name);
        if let Some(title) = &request.title {
            dialog = dialog.set_title(title);
        }
        let Some(handle) = dialog.save_file().await else {
            return Ok(None);
        };
        Ok(Some(FileSink::create(handle.path())?.handle()))
    })
}

pub(super) fn pick_writable_folder(
    options: FilePickerOptions,
) -> PickerFuture<Result<Option<String>, FilePickerError>> {
    Box::pin(async move {
        Ok(dialog(&options)
            .pick_folder()
            .await
            .map(|handle| handle.path().display().to_string()))
    })
}
