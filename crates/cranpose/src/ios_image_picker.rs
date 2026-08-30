#![allow(unsafe_code)]

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_services::{
    ImagePicker, ImagePickerError, ImageSource, PickerFuture, set_platform_image_picker,
};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, rc::Retained, runtime::AnyObject,
};
use objc2_foundation::{NSDictionary, NSObject, NSObjectProtocol};
use objc2_ui_kit::{
    UIImage, UIImagePickerController, UIImagePickerControllerDelegate,
    UIImagePickerControllerInfoKey, UIImagePickerControllerOriginalImage,
    UIImagePickerControllerSourceType, UINavigationControllerDelegate,
};

pub(crate) fn register() {
    set_platform_image_picker(Arc::new(IosImagePicker));
}

struct IosImagePicker;

impl ImagePicker for IosImagePicker {
    fn pick_image(
        &self,
        source: ImageSource,
    ) -> PickerFuture<Result<Option<Vec<u8>>, ImagePickerError>> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Box::pin(async {
                Err(ImagePickerError::Failed(
                    "the photo picker must be presented on the main thread".into(),
                ))
            });
        };
        match present(source, mtm) {
            Ok(future) => Box::pin(future),
            Err(error) => Box::pin(async move { Err(error) }),
        }
    }
}

type PickResult = Result<Option<Vec<u8>>, ImagePickerError>;

type SharedSlot = crate::ios_pick_future::SharedPickSlot<PickResult>;

type PickFuture = crate::ios_pick_future::PickFuture<PickResult, PickerDelegate>;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeImagePickerDelegate"]
    #[ivars = SharedSlot]
    struct PickerDelegate;

    unsafe impl NSObjectProtocol for PickerDelegate {}

    unsafe impl UINavigationControllerDelegate for PickerDelegate {}

    unsafe impl UIImagePickerControllerDelegate for PickerDelegate {
        #[unsafe(method(imagePickerController:didFinishPickingMediaWithInfo:))]
        fn did_finish(
            &self,
            picker: &UIImagePickerController,
            info: &NSDictionary<UIImagePickerControllerInfoKey, AnyObject>,
        ) {
            let result = extract_png_bytes(info);
            dismiss(picker);
            self.resolve(result);
        }

        #[unsafe(method(imagePickerControllerDidCancel:))]
        fn did_cancel(&self, picker: &UIImagePickerController) {
            dismiss(picker);
            self.resolve(Ok(None));
        }
    }
);

impl PickerDelegate {
    fn new(slot: SharedSlot, mtm: MainThreadMarker) -> Retained<Self> {
        crate::ios_pick_future::new_pick_delegate(slot, mtm)
    }

    fn resolve(&self, result: PickResult) {
        crate::ios_pick_future::resolve_pick_slot(self.ivars(), result);
    }
}

fn extract_png_bytes(info: &NSDictionary<UIImagePickerControllerInfoKey, AnyObject>) -> PickResult {
    // SAFETY: `UIImagePickerControllerOriginalImage` is an immutable framework
    // constant key.
    let key = unsafe { UIImagePickerControllerOriginalImage };
    let Some(object) = info.objectForKey(key) else {
        return Err(ImagePickerError::Failed("no image in picker result".into()));
    };
    let Ok(image) = object.downcast::<UIImage>() else {
        return Err(ImagePickerError::Failed(
            "picked item is not an image".into(),
        ));
    };
    match image.png_representation() {
        Some(data) => Ok(Some(data.to_vec())),
        None => Err(ImagePickerError::Failed(
            "could not encode the picked image".into(),
        )),
    }
}

fn dismiss(picker: &UIImagePickerController) {
    picker.dismissViewControllerAnimated_completion(true, None);
}

#[allow(deprecated)]
fn present(source: ImageSource, mtm: MainThreadMarker) -> Result<PickFuture, ImagePickerError> {
    let root = crate::ios_file_picker::root_view_controller(mtm).ok_or_else(|| {
        ImagePickerError::Failed("no root view controller to present from".into())
    })?;

    let source_type = match source {
        ImageSource::PhotoLibrary => UIImagePickerControllerSourceType::PhotoLibrary,
        ImageSource::Camera => UIImagePickerControllerSourceType::Camera,
    };
    if !UIImagePickerController::isSourceTypeAvailable(source_type, mtm) {
        return Err(ImagePickerError::Unsupported);
    }

    let picker = UIImagePickerController::new(mtm);
    picker.setSourceType(source_type);

    let slot: SharedSlot = Rc::new(RefCell::new(Default::default()));
    let delegate = PickerDelegate::new(slot.clone(), mtm);
    // SAFETY: the delegate conforms to both delegate protocols the picker needs;
    // `setDelegate` takes the untyped `id` and holds it weakly (the returned
    // future keeps the delegate alive).
    unsafe { picker.setDelegate(Some(&*delegate)) };
    root.presentViewController_animated_completion(&picker, true, None);

    Ok(PickFuture {
        slot,
        _delegate: delegate,
    })
}
