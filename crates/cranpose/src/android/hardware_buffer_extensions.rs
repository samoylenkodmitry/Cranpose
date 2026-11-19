//! The Vulkan device extensions with which a device imports the hardware buffers of a decoder.

use std::ffi::CStr;

// What the import needs: the extension that imports a hardware buffer, the family that acquires it
// from the decoder, the YCbCr conversion of the sampler, and what those two build on. Vulkan 1.1
// promoted the last four to the core, and a driver still lists them.
const EXTENSIONS: [&CStr; 7] = [
    c"VK_ANDROID_external_memory_android_hardware_buffer",
    c"VK_EXT_queue_family_foreign",
    c"VK_KHR_sampler_ycbcr_conversion",
    c"VK_KHR_external_memory",
    c"VK_KHR_dedicated_allocation",
    c"VK_KHR_bind_memory2",
    c"VK_KHR_get_memory_requirements2",
];

// The extensions to add to the ones a device already enables so that it imports hardware buffers,
// or `None` when the GPU's driver lacks one of them. `supported` answers whether the driver lists
// an extension.
pub(crate) fn extensions_to_add(
    supported: impl Fn(&CStr) -> bool,
    enabled: &[&CStr],
) -> Option<Vec<&'static CStr>> {
    if !EXTENSIONS.iter().all(|&wanted| supported(wanted)) {
        return None;
    }
    Some(
        EXTENSIONS
            .iter()
            .filter(|wanted| !enabled.contains(wanted))
            .copied()
            .collect(),
    )
}

// Whether a device that enables `enabled` imports hardware buffers.
pub(crate) fn imports(enabled: &[&CStr]) -> bool {
    EXTENSIONS.iter().all(|wanted| enabled.contains(wanted))
}

#[cfg(test)]
#[path = "tests/hardware_buffer_extensions_tests.rs"]
mod tests;
