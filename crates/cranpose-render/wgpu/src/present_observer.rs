/// Platform code the present thread runs around every present to a surface,
/// for what only the platform can do with its swapchain, such as asking the
/// display when earlier frames reached the screen.
pub trait PresentObserver: Send {
    /// Runs just before `surface` presents a frame.
    fn before_present(&mut self, surface: &wgpu::Surface<'static>);
    /// Runs just after `surface` presented it.
    fn after_present(&mut self, surface: &wgpu::Surface<'static>);

    /// Runs before `surface` is configured anew, while its current swapchain
    /// still stands. Does nothing unless the platform needs it to.
    fn before_reconfigure(&mut self, _surface: &wgpu::Surface<'static>) {}
}
