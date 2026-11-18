//! The decoder of a platform that has none. Its types have no values.

use std::{
    convert::Infallible,
    ffi::c_void,
    task::{Context, Poll},
};

use super::{AccessUnit, DecoderConfig, DecoderError, DecoderStats, FrameInfo, VideoFrame};

pub(super) struct Backend(Infallible);

#[derive(Clone, PartialEq)]
pub(super) struct Frames(Infallible);

pub(super) struct Image(Infallible);

impl Backend {
    pub(super) fn new(_config: DecoderConfig) -> Result<Self, DecoderError> {
        Err(DecoderError::Unavailable)
    }

    pub(super) fn submit(&mut self, _unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        match self.0 {}
    }

    pub(super) fn frames(&self) -> Frames {
        match self.0 {}
    }
}

impl Frames {
    pub(super) fn next_frame(&self) -> Option<VideoFrame> {
        match self.0 {}
    }

    pub(super) fn latest_frame(&self) -> Option<VideoFrame> {
        match self.0 {}
    }

    pub(super) fn poll_ready(&self, _context: &mut Context<'_>) -> Poll<()> {
        match self.0 {}
    }

    pub(super) fn next_report(&self) -> Option<FrameInfo> {
        match self.0 {}
    }

    pub(super) fn stats(&self) -> DecoderStats {
        match self.0 {}
    }
}

impl Image {
    pub(super) fn hardware_buffer(&self) -> *mut c_void {
        match self.0 {}
    }
}
