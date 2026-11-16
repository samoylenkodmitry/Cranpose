// What the page tells the XR tab: whether the browser can start an immersive session, whether
// one runs, and the headset's eyes before each of its frames. The page's script owns
// the WebXR session; the web platform crate passes its calls on to these functions.

use std::cell::Cell;

use cranpose_core::{mutableStateOf, MutableState};

use super::eye_view::EyeView;

// The states live as long as the app's thread. A scene that reads the eyes when it is drawn is
// drawn again when the page sets new ones, without a new composition.
#[derive(Clone, Copy)]
struct Page {
    views: MutableState<Option<[EyeView; 2]>>,
    presenting: MutableState<bool>,
    entry: MutableState<Option<fn()>>,
}

thread_local! {
    static PAGE: Cell<Option<Page>> = const { Cell::new(None) };
}

fn page() -> Page {
    PAGE.with(|cell| {
        cell.get().unwrap_or_else(|| {
            let page = Page {
                views: mutableStateOf(None),
                presenting: mutableStateOf(false),
                entry: mutableStateOf(None),
            };
            cell.set(Some(page));
            page
        })
    })
}

// Sets the function that starts an immersive session, or `None` where the browser has none.
// The XR tab shows "Enter VR" while one is set.
pub fn set_immersive_entry(enter: Option<fn()>) {
    page().entry.set(enter);
}

pub(crate) fn immersive_entry() -> Option<fn()> {
    page().entry.get()
}

// Tells the demo that an immersive session started or ended. While one runs, the demo fills
// the window with the two eye views that the page copies into the headset.
pub fn set_presenting(on: bool) {
    page().presenting.set(on);
    if !on {
        page().views.set(None);
    }
}

pub(crate) fn presenting() -> bool {
    page().presenting.get()
}

// Sets the headset's eyes for the next frame: for the left eye, then the right, its position
// (3 values), its orientation as a quaternion x, y, z, w (4) and the tangents of its view's
// left, right, up and down edges (4), in WebXR's reference space. Ignores a shorter slice.
pub fn set_views(data: &[f32]) {
    let (Some(left), Some(right)) = (data.get(0..11), data.get(11..22)) else {
        return;
    };
    page()
        .views
        .set(Some([EyeView::from_page(left), EyeView::from_page(right)]));
}

pub(super) fn headset_views() -> Option<[EyeView; 2]> {
    page().views.get()
}
