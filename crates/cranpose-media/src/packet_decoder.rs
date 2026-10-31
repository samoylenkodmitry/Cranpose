use symphonia::core::{
    audio::{AsGenericAudioBufferRef, AudioBuffer, GenericAudioBufferRef},
    codecs::{
        CodecInfo,
        audio::{AudioCodecParameters, AudioDecoder, FinalizeResult},
    },
    errors::{Error, Result},
    packet::PacketRef,
};

pub(crate) trait PacketCodec: Send + Sync {
    const INFO: CodecInfo;
    fn reset(&mut self);
    fn decode(&mut self, bytes: &[u8], output: &mut AudioBuffer<f32>) -> Result<()>;
}

pub(crate) struct PacketDecoder<C> {
    codec: C,
    params: AudioCodecParameters,
    output: AudioBuffer<f32>,
}

impl<C> PacketDecoder<C> {
    pub(crate) fn new(codec: C, params: AudioCodecParameters, output: AudioBuffer<f32>) -> Self {
        Self {
            codec,
            params,
            output,
        }
    }
}

impl<C: PacketCodec> AudioDecoder for PacketDecoder<C> {
    fn reset(&mut self) {
        self.codec.reset();
        self.output.clear();
    }
    fn codec_info(&self) -> &CodecInfo {
        &C::INFO
    }
    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }
    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> Result<GenericAudioBufferRef<'_>> {
        self.output.clear();
        self.codec.decode(packet.data, &mut self.output)?;
        let start = usize::try_from(packet.trim_start.get())
            .map_err(|_| Error::DecodeError("invalid audio packet trim"))?;
        let end = usize::try_from(packet.trim_end.get())
            .map_err(|_| Error::DecodeError("invalid audio packet trim"))?;
        self.output.trim(start, end);
        Ok(self.output.as_generic_audio_buffer_ref())
    }
    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }
    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.output.as_generic_audio_buffer_ref()
    }
}
