//! A streamed scene outside a headset: the left eye of the newest frame, in the layout.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose::{
    ContentScale, CropRect, DecodedFrames, FramePresenter, PlayerSurfaceIntake, VideoFrame,
    prelude::*,
};

use crate::{StreamCounts, StreamStats, StreamedViews, stream_source::StreamSource};

// What a flat scene keeps while it is in the composition.
struct Flat {
    // The source of the latest composition
    source: RefCell<Rc<StreamSource>>,
    // The totals, which an effect that starts again does not reset
    counts: Cell<StreamCounts>,
    // Whether the latest frame's views were refused, so that the log says it once
    refused: Cell<bool>,
    // Whether the log has said that a frame is foveated
    foveated: Cell<bool>,
}

// The left eye's picture of the newest frame, in the box that the layout gives it, scaled to fit.
#[composable]
pub(crate) fn FlatStream(
    frames: DecodedFrames,
    views: impl Fn(u64) -> StreamedViews + 'static,
    stats: StreamStats,
) {
    let source = Rc::new(StreamSource {
        frames,
        views: Rc::new(views),
        stats,
    });
    let flat = remember({
        let source = Rc::clone(&source);
        move || {
            Rc::new(Flat {
                source: RefCell::new(source),
                counts: Cell::new(StreamCounts::default()),
                refused: Cell::new(false),
                foveated: Cell::new(false),
            })
        }
    })
    .with(Rc::clone);
    let frames = source.frames.clone();
    // The effect that takes the frames reads the latest views and stats, as the headset does
    *flat.source.borrow_mut() = source;
    PlayerSurfaceIntake(
        frames,
        Modifier::empty().fill_max_size(),
        ContentScale::Fit,
        move |frames, presenter| flat.intake(frames, presenter),
    );
}

impl Flat {
    // Takes the pictures that the decoder has output, with the views that the app gives each. The
    // newest picture that the views accept is shown, and every other one is skipped.
    fn intake(&self, frames: &DecodedFrames, presenter: &mut FramePresenter) {
        let source = Rc::clone(&self.source.borrow());
        let mut counts = self.counts.get();
        let mut newest = None;
        while let Some(frame) = frames.next_frame() {
            match self.part(&source, &frame) {
                Some(part) => {
                    counts.skipped += u64::from(newest.replace((frame, part)).is_some());
                }
                None => counts.skipped += 1,
            }
        }
        if let Some((frame, part)) = newest {
            if presenter.present(frame, Some(part)).is_ok() {
                counts.shown += 1;
            } else {
                counts.skipped += 1;
            }
        }
        self.counts.set(counts);
        source.stats.publish(counts);
    }

    // The part of the frame that holds the left eye's picture, or `None` when the app's views for
    // the frame are refused.
    fn part(&self, source: &StreamSource, frame: &VideoFrame) -> Option<CropRect> {
        let crop = frame.crop();
        let views = (source.views)(frame.info().metadata).checked((crop.width, crop.height));
        match views {
            Ok(views) => {
                self.refused.set(false);
                if views.left.foveation.is_some() && !self.foveated.replace(true) {
                    log::warn!(
                        "cranpose xr: a streamed frame is foveated, and a flat scene shows its \
                         picture as the host encoded it"
                    );
                }
                Some(views.left.rect)
            }
            Err(reason) => {
                if !self.refused.replace(true) {
                    log::warn!("cranpose xr: the views of a streamed frame are refused: {reason}");
                }
                None
            }
        }
    }
}
