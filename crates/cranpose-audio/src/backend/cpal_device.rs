use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cranpose_services::AudioError;

use crate::{
    backend::{AudioSink, Renderer},
    mixer::RenderStatus,
};

const SCRATCH_SAMPLES: usize = 8192;

pub(crate) fn open(mut renderer: Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        AudioError::Backend("the system reports no default audio output device".to_string())
    })?;
    let supported = device
        .default_output_config()
        .map_err(|error| AudioError::Backend(format!("no usable output configuration: {error}")))?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = usize::from(config.channels).max(1);
    let sample_rate = config.sample_rate as f32;
    renderer.set_device_format(sample_rate, channels);

    let error_callback = |error| log::warn!("cranpose audio stream error: {error}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let _ = renderer.render(data);
            },
            error_callback,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut scratch = vec![0.0f32; SCRATCH_SAMPLES];
            device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let _ = render_into_integer(
                        &mut *renderer,
                        &mut scratch,
                        data,
                        channels,
                        |sample| (sample * f32::from(i16::MAX)) as i16,
                    );
                },
                error_callback,
                None,
            )
        }
        other => return Err(unwritable_sample_format(other)),
    }
    .map_err(|error| AudioError::Backend(format!("failed to build the output stream: {error}")))?;

    stream.play().map_err(|error| {
        AudioError::Backend(format!("failed to start the output stream: {error}"))
    })?;

    log::debug!("cranpose audio: cpal stream at {sample_rate} Hz, {channels} channels");
    Ok(Box::new(CpalSink { stream }))
}

fn unwritable_sample_format(format: cpal::SampleFormat) -> AudioError {
    AudioError::Backend(format!(
        "the default output device wants {format:?} samples, which the engine does not write"
    ))
}

fn render_into_integer<T>(
    renderer: &mut dyn Renderer,
    scratch: &mut [f32],
    data: &mut [T],
    channels: usize,
    convert: impl Fn(f32) -> T,
) -> RenderStatus {
    let chunk = (scratch.len() / channels) * channels;
    if chunk == 0 {
        return RenderStatus::Continue;
    }
    let mut status = RenderStatus::Continue;
    let mut offset = 0;
    while offset < data.len() {
        let take = (data.len() - offset).min(chunk);
        status = renderer.render(&mut scratch[..take]);
        for index in 0..take {
            data[offset + index] = convert(scratch[index]);
        }
        offset += take;
    }
    status
}

struct CpalSink {
    stream: cpal::Stream,
}

impl AudioSink for CpalSink {
    fn suspend(&self) {
        if let Err(error) = self.stream.pause() {
            log::warn!("failed to pause the output stream: {error}");
        }
    }

    fn resume(&self) {
        if let Err(error) = self.stream.play() {
            log::warn!("failed to restart the output stream: {error}");
        }
    }

    fn park(&self) {
        if let Err(error) = self.stream.pause() {
            log::warn!("failed to release the idle output stream: {error}");
        }
    }
}
