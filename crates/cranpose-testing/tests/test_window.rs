// The window of a test shell that draws a scene.

use cranpose_app_shell::AppShell;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::Size;

// Sizes the window of `shell` to `size`: the viewport in logical pixels and
// the buffer in whole pixels.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a window's size is not negative, and its buffer holds whole pixels"
)]
pub(crate) fn size_window(shell: &mut AppShell<TestRenderer>, size: Size) {
    shell.set_viewport(size.width, size.height);
    shell.set_buffer_size(size.width as u32, size.height as u32);
}

// Sizes the window of `shell` to `size` and draws its first frame at 16 ms,
// whose time in nanoseconds it returns.
pub(crate) fn first_frame(shell: &mut AppShell<TestRenderer>, size: Size) -> u64 {
    size_window(shell, size);
    let frame = 16_000_000;
    shell.update_at_frame_time_nanos(frame);
    frame
}
