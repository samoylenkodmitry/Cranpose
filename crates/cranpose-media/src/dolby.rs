use symphonia::core::{
    audio::{Audio, AudioBuffer, AudioMut, AudioSpec},
    codecs::{CodecInfo, audio::AudioCodecParameters},
    errors::{Error, Result},
};

use crate::packet_decoder::{PacketCodec, PacketDecoder};

pub(crate) struct Dolby {
    decoder: filmcraft_ac3::Decoder,
}

impl Dolby {
    pub(crate) fn new(params: &AudioCodecParameters) -> Result<PacketDecoder<Self>> {
        let rate = params
            .sample_rate
            .ok_or(Error::DecodeError("missing Dolby sample rate"))?;
        let channels = params
            .channels
            .clone()
            .ok_or(Error::DecodeError("missing Dolby channel layout"))?;
        Ok(PacketDecoder::new(
            Self {
                decoder: filmcraft_ac3::Decoder::new(),
            },
            params.clone(),
            AudioBuffer::new(AudioSpec::new(rate, channels), 1536),
        ))
    }

    fn packet_frames(mut bytes: &[u8], output: &AudioBuffer<f32>) -> Result<usize> {
        let mut frames: usize = 0;
        while !bytes.is_empty() {
            let header = filmcraft_ac3::parse_header(bytes).map_err(decode_error)?;
            bytes = bytes
                .get(header.frame_bytes..)
                .ok_or(Error::DecodeError("truncated Dolby frame"))?;
            if header.is_primary() {
                if header.sample_rate != output.spec().rate()
                    || header.channels() != output.spec().channels().count()
                {
                    return Err(Error::Unsupported("Dolby format changes within a track"));
                }
                frames = frames
                    .checked_add(header.samples())
                    .filter(|n| *n <= 1536 * 256)
                    .ok_or(Error::DecodeError("Dolby packet exceeds 256 frames"))?;
            }
        }
        Ok(frames)
    }
}

impl PacketCodec for Dolby {
    const INFO: CodecInfo = CodecInfo {
        short_name: "ac3/eac3",
        long_name: "Dolby Digital / Dolby Digital Plus",
        profiles: &[],
    };
    fn reset(&mut self) {
        self.decoder.reset();
    }
    fn decode(&mut self, mut bytes: &[u8], output: &mut AudioBuffer<f32>) -> Result<()> {
        let frames = Self::packet_frames(bytes, output)?;
        if frames > output.capacity() {
            *output = AudioBuffer::new(output.spec().clone(), frames);
        }
        output.render_uninit(Some(frames));
        let mut offset = 0;
        while !bytes.is_empty() {
            let decoded = self.decoder.decode(bytes).map_err(decode_error)?;
            bytes = &bytes[decoded.header.frame_bytes..];
            if !decoded.header.is_primary() {
                continue;
            }
            for (channel, samples) in decoded.channels.iter().enumerate() {
                let plane = output
                    .plane_mut(channel)
                    .ok_or(Error::DecodeError("invalid Dolby output channel"))?;
                plane[offset..offset + samples.len()].copy_from_slice(samples);
            }
            offset += decoded.header.samples();
        }
        Ok(())
    }
}

fn decode_error(error: filmcraft_ac3::Error) -> Error {
    match error {
        filmcraft_ac3::Error::Unsupported(_) => Error::Unsupported("unsupported Dolby coding mode"),
        _ => Error::DecodeError("invalid Dolby frame"),
    }
}

pub(crate) fn initialize(
    format: &mut dyn symphonia::core::formats::FormatReader,
    track_id: u32,
    params: &mut AudioCodecParameters,
) -> Result<Option<symphonia::core::packet::Packet>> {
    use symphonia::core::codecs::audio::well_known::{CODEC_ID_AC3, CODEC_ID_EAC3};
    if !matches!(params.codec, CODEC_ID_AC3 | CODEC_ID_EAC3) {
        return Ok(None);
    }
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track_id {
            continue;
        }
        let header = filmcraft_ac3::parse_header(&packet.data).map_err(decode_error)?;
        params.sample_rate = Some(header.sample_rate);
        params.channels = Some(symphonia::core::audio::Channels::Discrete(
            header.channels() as u16,
        ));
        return Ok(Some(packet));
    }
    Err(Error::DecodeError("Dolby track has no audio packets"))
}
