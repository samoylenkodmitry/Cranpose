// The window of a test shell that draws a scene.

use cranpose_app_shell::AppShell;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::Size;

// Sizes the window of `shell` to `size` and draws its first frame at 16 ms,
// whose time in nanoseconds it returns.
pub(crate) fn first_frame(shell: &mut AppShell<TestRenderer>, size: Size) -> u64 {
    shell.set_viewport(size.width, size.height);
    shell.set_buffer_size(size.width as u32, size.height as u32);
    let frame = 16_000_000;
    shell.update_at_frame_time_nanos(frame);
    frame
}
