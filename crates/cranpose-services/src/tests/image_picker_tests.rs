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
