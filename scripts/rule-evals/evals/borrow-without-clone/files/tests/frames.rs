use std::sync::Arc;

use frame::{Device, Renderer};

#[test]
fn a_frame_draws_what_was_queued_and_uploads_new_glyphs_once() {
    let mut renderer = Renderer::new(Arc::new(Device::new("test")));
    renderer.queue_text("xy");
    renderer.queue_text("yx");
    assert_eq!(renderer.render_frame(), 2);
    renderer.queue_text("xyz");
    assert_eq!(renderer.render_frame(), 1);
    assert_eq!(renderer.render_frame(), 0);
    assert_eq!(renderer.drawn(), ["0:xy", "0:yx", "1:xyz"]);
    assert_eq!(renderer.frames(), 3);
}
