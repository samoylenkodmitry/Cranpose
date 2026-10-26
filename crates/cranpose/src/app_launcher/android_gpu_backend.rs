/// Graphics API used by the Android WGPU renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AndroidGpuBackend {
    /// Render through Vulkan.
    #[default]
    Vulkan,
    /// Render through OpenGL ES.
    OpenGlEs,
}

#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
impl AndroidGpuBackend {
    pub(crate) fn with_override(self, value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("vulkan") => Self::Vulkan,
            Some(value)
                if ["gl", "gles", "opengl"]
                    .iter()
                    .any(|name| value.eq_ignore_ascii_case(name)) =>
            {
                Self::OpenGlEs
            }
            _ => self,
        }
    }

    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
    pub(crate) fn wgpu_backends(self) -> wgpu::Backends {
        match self {
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::OpenGlEs => wgpu::Backends::GL,
        }
    }
}
