use opus_pure::OpusMSDecoder;
use symphonia::core::{
    audio::{AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef},
    codecs::{
        CodecInfo,
        audio::{AudioCodecParameters, AudioDecoder, FinalizeResult},
    },
    errors::{Error, Result},
    packet::PacketRef,
};

const MAX_FRAMES: usize = 5_760;
const INFO: CodecInfo = CodecInfo {
    short_name: "opus",
    long_name: "Opus",
    profiles: &[],
};

// Opus family 1 uses Vorbis order; Symphonia uses ascending speaker positions.
const CHANNEL_ORDER: [&[usize]; 8] = [
    &[0],
    &[0, 1],
    &[0, 2, 1],
    &[0, 1, 2, 3],
    &[0, 2, 1, 3, 4],
    &[0, 2, 1, 5, 3, 4],
    &[0, 2, 1, 6, 5, 3, 4],
    &[0, 2, 1, 7, 5, 6, 3, 4],
];

pub(crate) struct Opus {
    decoder: OpusMSDecoder,
    params: AudioCodecParameters,
    pcm: Vec<f32>,
    output: AudioBuffer<f32>,
    gain: f32,
}

impl Opus {
    pub(crate) fn new(params: &AudioCodecParameters) -> Result<Self> {
        let header = params
            .extra_data
            .as_deref()
            .ok_or(Error::DecodeError("Opus header is missing"))?;
        if header.len() < 19 || &header[..8] != b"OpusHead" || header[8] > 15 {
            return Err(Error::DecodeError("invalid Opus header"));
        }
        let channels = usize::from(header[9]);
        let decoder = OpusMSDecoder::new(48_000, channels, header[18])
            .map_err(|_| Error::Unsupported("unsupported Opus channel layout"))?;
        let layout = decoder.layout();
        if header[18] != 0
            && (header.len() < 21 + channels
                || usize::from(header[19]) != layout.nb_streams
                || usize::from(header[20]) != layout.nb_coupled_streams
                || header[21..21 + channels] != layout.mapping)
        {
            return Err(Error::Unsupported("nonstandard Opus channel mapping"));
        }
        let positions = params
            .channels
            .clone()
            .filter(|positions| positions.count() == channels)
            .ok_or(Error::DecodeError("inconsistent Opus channel count"))?;
        let gain_q8 = i16::from_le_bytes([header[16], header[17]]);
        let mut params = params.clone();
        params.sample_rate = Some(48_000);
        Ok(Self {
            decoder,
            params,
            pcm: vec![0.0; MAX_FRAMES * channels],
            output: AudioBuffer::new(AudioSpec::new(48_000, positions), MAX_FRAMES),
            gain: 10.0_f32.powf(f32::from(gain_q8) / (20.0 * 256.0)),
        })
    }
}

impl AudioDecoder for Opus {
    fn reset(&mut self) {
        let _ = self.decoder.reset_state();
        self.output.clear();
    }

    fn codec_info(&self) -> &CodecInfo {
        &INFO
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }

    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> Result<GenericAudioBufferRef<'_>> {
        self.output.clear();
        let frames = self
            .decoder
            .decode(packet.data, MAX_FRAMES, &mut self.pcm)
            .map_err(|_| Error::DecodeError("invalid Opus packet"))?;
        self.output.render_uninit(Some(frames));
        let channels = self.decoder.channels();
        for (destination, &source) in CHANNEL_ORDER[channels - 1].iter().enumerate() {
            if let Some(plane) = self.output.plane_mut(destination) {
                for (frame, sample) in plane.iter_mut().enumerate() {
                    *sample = self.pcm[frame * channels + source] * self.gain;
                }
            }
        }
        self.output.trim(
            packet.trim_start.get() as usize,
            packet.trim_end.get() as usize,
        );
        Ok(self.output.as_generic_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.output.as_generic_audio_buffer_ref()
    }
}
