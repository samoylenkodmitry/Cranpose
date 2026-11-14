// The presentation constraints of the window a robot drives.

/// Presentation constraints of the window controlled by a robot.
#[derive(Debug, Clone)]
pub struct RobotPresentationInfo {
    /// Explicit environment request, if one was provided.
    pub requested_mode: Option<String>,
    /// Concrete surface mode after resolving automatic fallback choices.
    pub present_mode: wgpu::PresentMode,
    /// Concrete modes advertised by this surface.
    pub supported_modes: Vec<wgpu::PresentMode>,
    /// Application scheduling policy.
    pub frame_pacing_mode: cranpose_app_shell::FramePacingMode,
    /// Display refresh estimate used by the window scheduler.
    pub refresh_rate_hz: f64,
    /// The window surface's width and height in physical pixels, the unit
    /// of the rects of a frame's window damage.
    pub surface_size: (u32, u32),
}

impl RobotPresentationInfo {
    /// Whether the configured scheduling and presentation modes request unpaced rendering.
    ///
    /// This does not establish measured throughput; surface acquisition can still block.
    pub fn uses_unpaced_configuration(&self) -> bool {
        self.frame_pacing_mode == cranpose_app_shell::FramePacingMode::NoVsync
            && matches!(
                self.present_mode,
                wgpu::PresentMode::Immediate | wgpu::PresentMode::Mailbox
            )
    }
}
