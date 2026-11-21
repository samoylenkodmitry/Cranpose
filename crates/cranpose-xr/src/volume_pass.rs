//! One eye's draw of the scene that holds the volumes.

use cranpose::GuestView;
use cranpose_core::NodeId;

use crate::{eye_camera::EyeCamera, space::Space, volume_window::VolumeWindow};

// The texture that an eye's picture goes into, with the depth that the picture leaves.
pub(crate) struct EyeImage<'a> {
    pub(crate) color: &'a wgpu::Texture,
    pub(crate) color_view: &'a wgpu::TextureView,
    // A view of a texture of the scene's depth format with the color's size, or `None` for a
    // picture that leaves no depth
    pub(crate) depth: Option<&'a wgpu::TextureView>,
}

// The window of the composition that holds the scene of the volumes, which the guest view draws
// into the eyes' images. The pass finds the window root when the composition changes the set of
// its window roots, so a frame that changes none costs one comparison.
#[derive(Default)]
pub(crate) struct VolumePass {
    // The revision of the composition's window roots that `root` was found at
    seen: Option<u64>,
    // The scene's window root, if the composition has one
    root: Option<NodeId>,
    // The size of the pictures that the window root's view draws, once it has one
    opened: Option<(u32, u32)>,
}

impl VolumePass {
    // The scene's window root, once a draw found it.
    pub(crate) fn root(&self) -> Option<NodeId> {
        self.root
    }

    // Draws the scene as `eye` sees it into `image`. Returns whether the scene drew: it has no
    // window root while the app has no volume, and a pass that finds none draws nothing.
    pub(crate) fn draw(
        &mut self,
        guest: &mut GuestView,
        space: &Space,
        eye: &EyeCamera,
        image: &EyeImage<'_>,
    ) -> Result<bool, String> {
        let size = (image.color.width(), image.color.height());
        let Some(root) = self.find(guest) else {
            return Ok(false);
        };
        if self.opened != Some(size) {
            guest
                .open_window(root, size, 1.0)
                .map_err(|error| error.to_string())?;
            self.opened = Some(size);
        }
        // The camera is a state that the scene reads while it draws, so the guest view draws the
        // same composition again from this eye with nothing composed
        space.volumes().camera().set(eye.camera);
        guest
            .draw_window(root, image.color, image.color_view, image.depth)
            .map_err(|error| error.to_string())?;
        Ok(true)
    }

    // Closes the view of the scene's window root: its renderer, which holds the scene's pipelines
    // and targets, goes. The next draw finds the window root again.
    pub(crate) fn release(&mut self, guest: &mut GuestView) {
        if let Some(root) = self.root.take() {
            guest.close_window(root);
        }
        self.opened = None;
        self.seen = None;
    }

    // The scene's window root, found again when the composition's window roots changed. A window
    // root that left drops its view.
    fn find(&mut self, guest: &mut GuestView) -> Option<NodeId> {
        let revision = guest.shell().window_roots_revision();
        if self.seen != Some(revision) {
            self.seen = Some(revision);
            let found = guest
                .shell()
                .window_roots()
                .into_iter()
                .find(|entry| entry.descriptor.as_any().is::<VolumeWindow>())
                .map(|entry| entry.node);
            if found != self.root {
                if let Some(old) = self.root {
                    guest.close_window(old);
                }
                self.opened = None;
                self.root = found;
            }
        }
        self.root
    }
}
