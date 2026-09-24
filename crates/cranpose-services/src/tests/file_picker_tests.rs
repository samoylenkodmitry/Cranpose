use super::*;

#[test]
fn options_builder_sets_title_and_filters() {
    let options = FilePickerOptions::default()
        .with_title("Pick audio")
        .with_filter(FileFilter::new("Audio", &["mp3", "flac"]).with_mime_types(&["audio/*"]));
    assert_eq!(options.title.as_deref(), Some("Pick audio"));
    assert_eq!(options.filters.len(), 1);
    assert_eq!(options.filters[0].extensions, vec!["mp3", "flac"]);
    assert_eq!(options.mime_types(), vec!["audio/*"]);
}

#[test]
fn default_picker_is_created() {
    let picker = default_file_picker();
    assert_eq!(Rc::strong_count(&picker), 1);
}

struct Marker;

impl FilePicker for Marker {
    fn pick_file(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
        Box::pin(async {
            Ok(Some(
                crate::content::BytesContent::named("marker.txt", b"marker".to_vec()).handle(),
            ))
        })
    }

    fn pick_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
        Box::pin(async { Ok(None) })
    }
}

#[test]
fn registered_platform_picker_takes_precedence() {
    clear_platform_file_picker();
    assert!(registered_platform_file_picker().is_none());
    set_platform_file_picker(Rc::new(Marker));
    assert!(registered_platform_file_picker().is_some());

    let picked = pollster::block_on(default_file_picker().pick_file(FilePickerOptions::default()))
        .expect("the marker picker resolves")
        .expect("the marker picker picks a file");
    assert_eq!(picked.metadata().name, "marker.txt");
    clear_platform_file_picker();
}

#[test]
fn multi_selection_falls_back_to_the_single_chooser() {
    clear_platform_file_picker();
    set_platform_file_picker(Rc::new(Marker));
    let picked = pollster::block_on(default_file_picker().pick_files(FilePickerOptions::default()))
        .expect("the marker picker resolves");
    assert_eq!(picked.len(), 1);
    clear_platform_file_picker();
}

#[test]
fn unsupported_operations_report_the_platform_gap() {
    clear_platform_file_picker();
    set_platform_file_picker(Rc::new(Marker));
    let saved = pollster::block_on(
        default_file_picker().save_document(SaveDocumentRequest::new("a.txt", "text/plain")),
    );
    let Err(error) = saved else {
        panic!("the marker picker offers no save destination");
    };
    assert_eq!(error, FilePickerError::UnsupportedPlatform);
    clear_platform_file_picker();
}
