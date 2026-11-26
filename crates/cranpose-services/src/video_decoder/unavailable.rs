//! The decoder of a platform that has none. Its types have no values.

use std::{convert::Infallible, ffi::c_void};

use super::{AccessUnit, DecoderConfig, DecoderError, frame_queue::Frames};

pub(super) struct Backend(Infallible);

pub(super) struct Image(Infallible);

impl Backend {
    pub(super) fn new(_config: DecoderConfig) -> Result<Self, DecoderError> {
        Err(DecoderError::Unavailable)
    }

    pub(super) fn submit(&mut self, _unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        match self.0 {}
    }

    pub(super) fn drain(&mut self) -> Result<(), DecoderError> {
        match self.0 {}
    }

    pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
        match self.0 {}
    }

    pub(super) fn frames(&self) -> Frames {
        match self.0 {}
    }
}

impl Image {
    pub(super) fn hardware_buffer(&self) -> *mut c_void {
        match self.0 {}
    }
}
