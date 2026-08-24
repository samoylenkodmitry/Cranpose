//! Photo/image picker: choose a single image from the user's photo library.
//!
//! On iOS "photos" and "files" are distinct — a scanner's document photos live
//! in the Photos library, which the file picker (`UIDocumentPicker`) cannot
//! reach. The iOS backend registers a `UIImagePickerController`-based photo
//! picker through [`set_platform_image_picker`]. The compiled-in default
//! delegates to the file picker with an image filter, which is the right
//! behavior on desktop (a file dialog), the web (`<input accept="image/*">`),
//! and Android (SAF), where a plain file picker already surfaces images.

use std::{cell::RefCell, sync::Arc};

use cranpose_core::{compositionLocalOfWithPolicy, CompositionLocal, CompositionLocalProvider};
use cranpose_macros::composable;

use crate::{
    file_picker::{default_file_picker, FileFilter, FilePickerOptions, PickerFuture},
    registry::ServiceRegistry,
};

/// Image extensions offered when the picker falls back to the file picker.
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "heic", "heif"];

#[derive(thiserror::Error, Debug)]
pub enum ImagePickerError {
    #[error("image picking is not supported on this platform")]
    Unsupported,
    #[error("failed to pick an image: {0}")]
    Failed(String),
}

/// Where an image comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageSource {
    /// The user's photo library (or a file dialog on platforms without one).
    PhotoLibrary,
    /// A photo captured with the device camera.
    Camera,
}

/// Picks or captures a single image and returns its encoded bytes.
pub trait ImagePicker: Send + Sync {
    /// Present a picker/camera for `source` and resolve to the chosen image's
    /// encoded bytes, or `None` if the user cancelled.
    fn pick_image(
        &self,
        source: ImageSource,
    ) -> PickerFuture<Result<Option<Vec<u8>>, ImagePickerError>>;
}

pub type ImagePickerRef = Arc<dyn ImagePicker>;

static PLATFORM_IMAGE_PICKER: ServiceRegistry<dyn ImagePicker> = ServiceRegistry::new();

/// Installs a platform image picker, replacing any previously installed one.
/// The iOS backend registers a photo-library picker here.
pub fn set_platform_image_picker(picker: ImagePickerRef) {
    PLATFORM_IMAGE_PICKER.set(picker);
}

/// Removes any registered platform image picker (tests and teardown).
pub fn clear_platform_image_picker() {
    PLATFORM_IMAGE_PICKER.clear();
}

fn registered_platform_image_picker() -> Option<ImagePickerRef> {
    PLATFORM_IMAGE_PICKER.get_or_warn("image picker")
}

struct PlatformImagePicker;

impl ImagePicker for PlatformImagePicker {
    fn pick_image(
        &self,
        source: ImageSource,
    ) -> PickerFuture<Result<Option<Vec<u8>>, ImagePickerError>> {
        if let Some(picker) = registered_platform_image_picker() {
            return picker.pick_image(source);
        }
        // No platform picker: a live camera needs one, but a library pick can
        // fall back to the file picker filtered to images.
        if source == ImageSource::Camera {
            return Box::pin(async { Err(ImagePickerError::Unsupported) });
        }
        Box::pin(async {
            let picker = default_file_picker();
            let options = FilePickerOptions::default()
                .with_title("Choose image")
                .with_filter(FileFilter::new("Images", IMAGE_EXTENSIONS));
            match picker.pick_file(options).await {
                Ok(Some(entry)) => match entry.read_all().await {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(error) => Err(ImagePickerError::Failed(error.to_string())),
                },
                Ok(None) => Ok(None),
                Err(error) => Err(ImagePickerError::Failed(error.to_string())),
            }
        })
    }
}

pub fn default_image_picker() -> ImagePickerRef {
    Arc::new(PlatformImagePicker)
}

pub fn local_image_picker() -> CompositionLocal<ImagePickerRef> {
    thread_local! {
        static LOCAL_IMAGE_PICKER: RefCell<Option<CompositionLocal<ImagePickerRef>>> = const { RefCell::new(None) };
    }

    LOCAL_IMAGE_PICKER.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_image_picker, Arc::ptr_eq))
            .clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub fn ProvideImagePicker(content: impl FnOnce()) {
    let picker = cranpose_core::remember(default_image_picker).with(|state| state.clone());
    let local = local_image_picker();

    CompositionLocalProvider(vec![local.provides(picker)], move || {
        content();
    });
}

#[cfg(test)]
mod tests {
    use std::sync::RwLock;

    use super::*;

    struct FixedImagePicker {
        bytes: RwLock<Option<Vec<u8>>>,
    }

    impl ImagePicker for FixedImagePicker {
        fn pick_image(
            &self,
            _source: ImageSource,
        ) -> PickerFuture<Result<Option<Vec<u8>>, ImagePickerError>> {
            let bytes = self.bytes.read().unwrap().clone();
            Box::pin(async move { Ok(bytes) })
        }
    }

    #[test]
    fn registered_image_picker_takes_precedence() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_image_picker();
        set_platform_image_picker(Arc::new(FixedImagePicker {
            bytes: RwLock::new(Some(vec![1, 2, 3])),
        }));
        let picker = default_image_picker();
        let result = pollster::block_on(picker.pick_image(ImageSource::Camera));
        assert_eq!(result.unwrap(), Some(vec![1, 2, 3]));
        clear_platform_image_picker();
    }

    #[test]
    fn camera_is_unsupported_without_a_platform_picker() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_image_picker();
        let result = pollster::block_on(default_image_picker().pick_image(ImageSource::Camera));
        assert!(matches!(result, Err(ImagePickerError::Unsupported)));
    }
}
