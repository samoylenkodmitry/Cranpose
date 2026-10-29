// A submit with no work, while the page draws no frames: the fence it places
// tells when the GPU has done what the device was handed since the last
// submit, as the link of a pipeline built between frames.
pub(crate) fn submit_nothing(queue: &wgpu::Queue) {
    queue.submit([]);
}
