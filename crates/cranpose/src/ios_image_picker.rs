//! iOS photo-library picker built on `UIImagePickerController`.
//!
//! Registered as the platform image picker (see
//! [`cranpose_services::set_platform_image_picker`]) by the iOS backend, so
//! apps can import a photo from the user's library (where camera photos live)
//! rather than only from Files via the document picker.
//!
//! The delegate callbacks run on the UIKit main thread, so the shared result
//! slot mirrors `ios_file_picker` (no cross-thread marshaling). The chosen
//! image is re-encoded to PNG bytes; the caller decodes it.
#![allow(unsafe_code)]

use cranpose_services::{
    set_platform_image_picker, ImagePicker, ImagePickerError, ImageSource, PickerFuture,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSDictionary, NSObject, NSObjectProtocol};
use objc2_ui_kit::{
    UIImage, UIImagePickerController, UIImagePickerControllerDelegate,
    UIImagePickerControllerInfoKey, UIImagePickerControllerOriginalImage,
    UIImagePickerControllerSourceType, UINavigationControllerDelegate,
};
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// Installs the iOS photo picker as the platform image picker.
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

#[derive(Default)]
struct PickSlot {
    result: Option<PickResult>,
    waker: Option<Waker>,
}

type SharedSlot = Rc<RefCell<PickSlot>>;

/// Future resolved when the delegate reports a pick or cancellation. Holds the
/// delegate alive (the controller keeps only a weak reference to it).
struct PickFuture {
    slot: SharedSlot,
    _delegate: Retained<PickerDelegate>,
}

impl Future for PickFuture {
    type Output = PickResult;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<PickResult> {
        let mut slot = self.slot.borrow_mut();
        if let Some(result) = slot.result.take() {
            Poll::Ready(result)
        } else {
            slot.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

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
        let this = Self::alloc(mtm).set_ivars(slot);
        unsafe { msg_send![super(this), init] }
    }

    fn resolve(&self, result: PickResult) {
        let mut slot = self.ivars().borrow_mut();
        slot.result = Some(result);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// Encode the picked original image to PNG bytes.
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

// `UIImagePickerController` is deprecated in favor of `PHPickerViewController`,
// but its delegate delivers the chosen `UIImage` synchronously on the main
// thread, matching `ios_file_picker`'s slot+waker model. PHPicker loads data
// asynchronously on a background queue, which needs cross-thread waker
// marshaling; adopt it once that path is verified on a device. The picker still
// works and is the pragmatic photo-library source today.
#[allow(deprecated)]
fn present(source: ImageSource, mtm: MainThreadMarker) -> Result<PickFuture, ImagePickerError> {
    let root = crate::ios_file_picker::root_view_controller(mtm).ok_or_else(|| {
        ImagePickerError::Failed("no root view controller to present from".into())
    })?;

    let source_type = match source {
        ImageSource::PhotoLibrary => UIImagePickerControllerSourceType::PhotoLibrary,
        ImageSource::Camera => UIImagePickerControllerSourceType::Camera,
    };
    // The camera is unavailable on the Simulator (and if the user denied access).
    if !UIImagePickerController::isSourceTypeAvailable(source_type, mtm) {
        return Err(ImagePickerError::Unsupported);
    }

    let picker = UIImagePickerController::new(mtm);
    picker.setSourceType(source_type);

    let slot: SharedSlot = Rc::new(RefCell::new(PickSlot::default()));
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
