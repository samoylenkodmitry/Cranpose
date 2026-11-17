//! The streamed scenes: a projection layer for each, from the frames of a decoder.
//!
//! A frame holds both eyes' pictures. The decoded frames wait in a small queue. At each of the
//! headset's frames the queue gives the frame whose target display time lies closest to the time
//! of the frame being submitted. That frame is drawn into a swapchain for each eye, and the layer
//! says which pose and field of view the frame was rendered with. The compositor reprojects the
//! pictures from those poses to the head's newer pose. While no other frame is chosen, the layer
//! shows the last pictures again.

use std::{rc::Rc, time::Duration};

use cranpose::{CropRect, VideoFrame};
use openxr as xr;

use crate::{
    StreamedView, StreamedViews,
    foveated_unwarp::FoveatedUnwarp,
    frame_queue::FrameQueue,
    openxr::{
        external_texture::{ExternalTexture, Pass, Target},
        graphics::{Api, Graphics},
        session::{ProjectionEye, ProjectionLayer, XrSession},
        stream_costs::Costs,
    },
    space::Space,
    stream_source::StreamSource,
};

// One eye's swapchain.
struct EyeTarget {
    swapchain: xr::Swapchain<Api>,
    size: (u32, u32),
    // The target of each image of the swapchain, by the image's index
    targets: Vec<Target>,
    // Whether the swapchain has an image to show
    shown: bool,
}

// A decoded frame with the views that the app gave it, which the compositor accepts.
struct Ready {
    frame: VideoFrame,
    views: StreamedViews,
}

// A streamed scene as the backend keeps it.
struct Stream {
    id: u64,
    queue: FrameQueue<Ready>,
    // The frame drawn last. The GPU may still read its buffer, so the frame stays until the next
    // one is drawn.
    held: Option<VideoFrame>,
    eyes: [Option<EyeTarget>; 2],
    // The views of the frame drawn last
    views: Option<StreamedViews>,
    // Whether the stream failed and shows nothing
    failed: bool,
    // Whether the latest frame's views were refused, so that the log says it once
    refused: bool,
}

pub(crate) struct StreamLayers {
    // `None` when the driver cannot import a hardware buffer: no stream shows
    importer: Option<ExternalTexture>,
    format: u32,
    streams: Vec<Stream>,
    costs: Costs,
}

impl StreamLayers {
    pub(crate) fn new(graphics: &Graphics, xr_session: &XrSession) -> Self {
        let mut costs = None;
        let importer = match ExternalTexture::new(graphics) {
            Ok(mut importer) => {
                costs = Some(Costs::new(&mut importer));
                Some(importer)
            }
            Err(error) => {
                log::warn!("cranpose xr: no streamed scenes: {error}");
                None
            }
        };
        // The shader writes linear light. An sRGB swapchain keeps the picture's own bytes, and
        // loses no precision in the darks.
        let formats = xr_session
            .session
            .enumerate_swapchain_formats()
            .unwrap_or_default();
        let [srgb, linear] = Graphics::FORMATS.map(|format| format.api);
        let format = if formats.contains(&srgb) {
            srgb
        } else {
            linear
        };
        Self {
            importer,
            format,
            streams: Vec::new(),
            costs: costs.unwrap_or_default(),
        }
    }

    // Drops the frames of each scene. While the runtime shows none of the app's layers no frame is
    // drawn, but a decoder whose frames wait would stop.
    pub(crate) fn skip(&mut self, space: &Space) {
        let sources = space.streams();
        self.sync(&sources);
        for (id, source) in sources {
            if let Some(stream) = self.streams.iter_mut().find(|stream| stream.id == id) {
                while source.frames.next_frame().is_some() {
                    stream.queue.skipped();
                }
                stream.queue.clear();
                source.stats.publish(stream.queue.counts());
            }
        }
    }

    // Takes the decoded frames of each scene into its queue, and draws the frame that suits
    // `display_time`, the time at which the headset's frame reaches the display. A frame that the
    // app's views refuse goes.
    pub(crate) fn update(&mut self, xr_session: &XrSession, space: &Space, display_time: Duration) {
        let sources = space.streams();
        self.sync(&sources);
        for (id, source) in sources {
            let Some(stream) = self.streams.iter_mut().find(|stream| stream.id == id) else {
                continue;
            };
            stream.intake(&source);
            if let (Some(importer), false) = (self.importer.as_mut(), stream.failed) {
                if let Some(ready) = stream.queue.choose(display_time) {
                    self.costs.begin(importer);
                    let drawn = draw_frame(importer, self.format, xr_session, stream, ready);
                    self.costs.end(importer);
                    if let Err(error) = drawn {
                        log::error!("cranpose xr: the streamed scene {id} ends: {error}");
                        stream.failed = true;
                    }
                }
            } else {
                stream.queue.clear();
            }
            source.stats.publish(stream.queue.counts());
        }
    }

    // Makes a stream for each scene that is new, and forgets the scenes that are gone.
    fn sync(&mut self, sources: &[(u64, Rc<StreamSource>)]) {
        if let Some(importer) = &self.importer {
            for stream in self
                .streams
                .iter_mut()
                .filter(|stream| !sources.iter().any(|(id, _)| *id == stream.id))
            {
                delete_targets(importer, stream);
            }
        }
        self.streams
            .retain(|stream| sources.iter().any(|(id, _)| *id == stream.id));
        for (id, _) in sources {
            if !self.streams.iter().any(|stream| stream.id == *id) {
                self.streams.push(Stream {
                    id: *id,
                    queue: FrameQueue::new(),
                    held: None,
                    eyes: [None, None],
                    views: None,
                    failed: false,
                    refused: false,
                });
            }
        }
    }

    // The layers of the scenes that have shown a frame.
    pub(crate) fn layers(&self) -> Vec<ProjectionLayer<'_>> {
        self.streams
            .iter()
            .filter(|stream| !stream.failed)
            .filter_map(|stream| {
                let views = stream.views?;
                let [Some(left), Some(right)] = &stream.eyes else {
                    return None;
                };
                (left.shown && right.shown).then(|| ProjectionLayer {
                    eyes: [
                        projection_eye(left, &views.left),
                        projection_eye(right, &views.right),
                    ],
                })
            })
            .collect()
    }
}

impl Drop for StreamLayers {
    fn drop(&mut self) {
        let Some(importer) = &self.importer else {
            return;
        };
        for stream in &mut self.streams {
            delete_targets(importer, stream);
        }
    }
}

// Deletes the framebuffers of the stream's swapchain images, which must go before the swapchains.
fn delete_targets(importer: &ExternalTexture, stream: &mut Stream) {
    for eye in stream.eyes.iter_mut().filter_map(Option::take) {
        for target in eye.targets {
            importer.delete_target(target);
        }
    }
}

impl Stream {
    // Takes the frames that the decoder has output into the queue, with the views that the app
    // gives each. A frame whose views the compositor would refuse is dropped.
    fn intake(&mut self, source: &StreamSource) {
        while let Some(frame) = source.frames.next_frame() {
            let crop = frame.crop();
            let picture = (crop.width, crop.height);
            match (source.views)(frame.info().metadata).checked(picture) {
                Ok(views) => {
                    self.refused = false;
                    self.queue
                        .push(Ready { frame, views }, views.target_display_time);
                }
                Err(reason) => {
                    if !self.refused {
                        log::warn!(
                            "cranpose xr: the views of a streamed frame are refused: {reason}"
                        );
                    }
                    self.refused = true;
                    self.queue.skipped();
                }
            }
        }
    }
}

// Draws the frame into the stream's swapchains and remembers its views.
fn draw_frame(
    importer: &mut ExternalTexture,
    format: u32,
    xr_session: &XrSession,
    stream: &mut Stream,
    ready: Ready,
) -> Result<(), String> {
    let Ready { frame, views } = ready;
    let crop = frame.crop();
    let eyes = [views.left, views.right];
    for (slot, view) in stream.eyes.iter_mut().zip(eyes) {
        // The swapchain holds the picture that the compositor shows: the full view of a
        // foveated picture
        let size = view.output_size();
        if slot.as_ref().is_none_or(|eye| eye.size != size) {
            if let Some(old) = slot.take() {
                for target in old.targets {
                    importer.delete_target(target);
                }
            }
            *slot = Some(make_eye(importer, format, xr_session, size)?);
        }
    }
    let [Some(left), Some(right)] = &mut stream.eyes else {
        return Ok(());
    };
    let indices = [left.acquire()?, right.acquire()?];
    let targets = [left.targets.get(indices[0]), right.targets.get(indices[1])];
    let [Some(left_target), Some(right_target)] = targets else {
        return Err("a stream image with no target".into());
    };
    // Both eyes' images are drawn from one EGL image of the frame's buffer
    let unwarps = eyes.map(|view| view.foveation.as_ref().map(FoveatedUnwarp::new));
    let pass = |target, eye: &EyeTarget, rect: CropRect, unwarp| Pass {
        target,
        size: eye.size,
        part: [crop.x + rect.x, crop.y + rect.y, rect.width, rect.height],
        unwarp,
    };
    importer.draw(
        frame.as_hardware_buffer_ptr(),
        frame.size(),
        &[
            pass(left_target, left, views.left.rect, unwarps[0].as_ref()),
            pass(right_target, right, views.right.rect, unwarps[1].as_ref()),
        ],
    )?;
    left.release()?;
    right.release()?;
    stream.views = Some(views);
    stream.held = Some(frame);
    Ok(())
}

fn make_eye(
    importer: &ExternalTexture,
    format: u32,
    xr_session: &XrSession,
    size: (u32, u32),
) -> Result<EyeTarget, String> {
    let swapchain = xr_session
        .session
        .create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::SAMPLED,
            format,
            sample_count: 1,
            width: size.0,
            height: size.1,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })
        .map_err(|error| format!("OpenXR stream swapchain of {size:?}: {error}"))?;
    let targets = swapchain
        .enumerate_images()
        .map_err(|error| format!("OpenXR stream swapchain images: {error}"))?
        .into_iter()
        .map(|name| importer.target(name))
        .collect::<Result<_, _>>()?;
    Ok(EyeTarget {
        swapchain,
        size,
        targets,
        shown: false,
    })
}

impl EyeTarget {
    // Takes the swapchain's next image and returns its index, once the compositor is done with
    // it.
    fn acquire(&mut self) -> Result<usize, String> {
        let index = self
            .swapchain
            .acquire_image()
            .map_err(|error| format!("OpenXR stream acquire: {error}"))?;
        self.swapchain
            .wait_image(xr::Duration::INFINITE)
            .map_err(|error| format!("OpenXR stream wait image: {error}"))?;
        Ok(index as usize)
    }

    // Hands the image that `acquire` took to the compositor.
    fn release(&mut self) -> Result<(), String> {
        self.swapchain
            .release_image()
            .map_err(|error| format!("OpenXR stream release: {error}"))?;
        self.shown = true;
        Ok(())
    }
}

fn projection_eye<'a>(eye: &'a EyeTarget, view: &StreamedView) -> ProjectionEye<'a> {
    ProjectionEye {
        swapchain: &eye.swapchain,
        size: eye.size,
        pose: view.pose,
        fov: view.fov,
    }
}
