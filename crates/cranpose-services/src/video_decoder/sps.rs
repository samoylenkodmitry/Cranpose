//! Minimal H.264 and HEVC SPS VUI parser.
//!
//! Reads color primaries, sample range, transfer function, and B-frame reorder depth straight
//! from Annex-B SPS NAL units without heap allocation so every decoder backend shares one
//! source of truth for stream metadata.

use super::{ColorPrimaries, ColorRange, ColorTransfer, VideoCodec};

// Metadata parsed from the active sequence parameter set of an elementary stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct StreamMetadata {
    pub(super) primaries: Option<ColorPrimaries>,
    pub(super) range: Option<ColorRange>,
    pub(super) transfer: Option<ColorTransfer>,
    pub(super) max_reorder_frames: Option<usize>,
}

impl StreamMetadata {
    // Parses an Annex-B SPS NAL payload (beginning with its NAL header byte, no start code).
    pub(super) fn parse_sps(codec: VideoCodec, nal: &[u8]) -> Option<Self> {
        match codec {
            VideoCodec::H264 => parse_h264_sps(nal),
            VideoCodec::Hevc => parse_hevc_sps(nal),
            VideoCodec::Av1 => None,
        }
    }
}

// Streaming RBSP bit reader that skips H.264/HEVC emulation-prevention 0x03 bytes inline.
struct BitReader<'a> {
    data: &'a [u8],
    byte_index: usize,
    zero_run: u8,
    current_byte: u8,
    bits_left: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_index: 0,
            zero_run: 0,
            current_byte: 0,
            bits_left: 0,
        }
    }

    fn next_rbsp_byte(&mut self) -> Option<u8> {
        while self.byte_index < self.data.len() {
            let byte = self.data[self.byte_index];
            self.byte_index += 1;
            if self.zero_run >= 2 && byte == 0x03 {
                self.zero_run = 0;
                continue;
            }
            self.zero_run = if byte == 0 {
                self.zero_run.saturating_add(1)
            } else {
                0
            };
            return Some(byte);
        }
        None
    }

    fn read_bit(&mut self) -> Option<u8> {
        if self.bits_left == 0 {
            self.current_byte = self.next_rbsp_byte()?;
            self.bits_left = 8;
        }
        self.bits_left -= 1;
        Some((self.current_byte >> self.bits_left) & 1)
    }

    fn read_flag(&mut self) -> Option<bool> {
        Some(self.read_bit()? != 0)
    }

    fn read_bits(&mut self, count: u8) -> Option<u32> {
        let mut value = 0_u32;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Some(value)
    }

    fn skip_bits(&mut self, count: usize) -> Option<()> {
        for _ in 0..count {
            self.read_bit()?;
        }
        Some(())
    }

    fn read_exp_golomb(&mut self) -> Option<u32> {
        let mut zeros = 0_u8;
        while self.read_bit()? == 0 {
            zeros += 1;
            if zeros >= 31 {
                return None;
            }
        }
        if zeros == 0 {
            return Some(0);
        }
        let suffix = self.read_bits(zeros)?;
        Some(((1_u32 << zeros) - 1) + suffix)
    }

    fn skip_exp_golombs(&mut self, count: usize) -> Option<()> {
        for _ in 0..count {
            self.read_exp_golomb()?;
        }
        Some(())
    }
}

fn parse_h264_sps(nal: &[u8]) -> Option<StreamMetadata> {
    let mut reader = BitReader::new(nal.get(1..)?);
    let profile_idc = reader.read_bits(8)?;
    reader.skip_bits(16)?;
    reader.read_exp_golomb()?;
    if is_h264_high_profile(profile_idc) {
        skip_h264_chroma_and_scaling(&mut reader)?;
    }
    reader.read_exp_golomb()?;
    skip_h264_poc(&mut reader)?;
    skip_h264_geometry(&mut reader)?;
    let mut meta = StreamMetadata::default();
    if reader.read_flag()? {
        parse_h264_vui(&mut reader, &mut meta)?;
    }
    Some(meta)
}

fn skip_h264_geometry(reader: &mut BitReader<'_>) -> Option<()> {
    reader.read_exp_golomb()?;
    reader.read_flag()?;
    reader.skip_exp_golombs(2)?;
    if !reader.read_flag()? {
        reader.read_flag()?;
    }
    reader.read_flag()?;
    if reader.read_flag()? {
        reader.skip_exp_golombs(4)?;
    }
    Some(())
}

fn is_h264_high_profile(profile_idc: u32) -> bool {
    matches!(
        profile_idc,
        44 | 83 | 86 | 100 | 110 | 118 | 122 | 128 | 134 | 135 | 138 | 139 | 244
    )
}

fn skip_h264_chroma_and_scaling(reader: &mut BitReader<'_>) -> Option<()> {
    let chroma_format_idc = reader.read_exp_golomb()?;
    if chroma_format_idc == 3 {
        reader.read_flag()?;
    }
    reader.skip_exp_golombs(2)?;
    reader.read_flag()?;
    if reader.read_flag()? {
        let count = if chroma_format_idc != 3 { 8 } else { 12 };
        for index in 0..count {
            if reader.read_flag()? {
                skip_h264_scaling_list(reader, if index < 6 { 16 } else { 64 })?;
            }
        }
    }
    Some(())
}

fn skip_h264_scaling_list(reader: &mut BitReader<'_>, size: usize) -> Option<()> {
    let mut last_scale = 8_i32;
    let mut next_scale = 8_i32;
    for _ in 0..size {
        if next_scale != 0 {
            let code = reader.read_exp_golomb()?;
            let delta = if code % 2 == 1 {
                code.div_ceil(2) as i32
            } else {
                -((code / 2) as i32)
            };
            next_scale = (last_scale + delta + 256) % 256;
        }
        if next_scale != 0 {
            last_scale = next_scale;
        }
    }
    Some(())
}

fn skip_h264_poc(reader: &mut BitReader<'_>) -> Option<()> {
    match reader.read_exp_golomb()? {
        0 => {
            reader.read_exp_golomb()?;
        }
        1 => {
            reader.read_flag()?;
            reader.skip_exp_golombs(2)?;
            let cycle = usize::try_from(reader.read_exp_golomb()?).ok()?;
            if cycle > 256 {
                return None;
            }
            reader.skip_exp_golombs(cycle)?;
        }
        _ => {}
    }
    Some(())
}

fn parse_h264_vui(reader: &mut BitReader<'_>, meta: &mut StreamMetadata) -> Option<()> {
    skip_vui_aspect_and_overscan(reader)?;
    parse_vui_video_signal_type(reader, meta)?;
    if reader.read_flag()? {
        reader.skip_exp_golombs(2)?;
    }
    if reader.read_flag()? {
        reader.skip_bits(65)?;
    }
    skip_h264_vui_hrd(reader)?;
    if reader.read_flag()? {
        reader.read_flag()?;
        reader.skip_exp_golombs(4)?;
        meta.max_reorder_frames = usize::try_from(reader.read_exp_golomb()?).ok();
    }
    Some(())
}

fn skip_h264_vui_hrd(reader: &mut BitReader<'_>) -> Option<()> {
    let nal_hrd = reader.read_flag()?;
    if nal_hrd {
        skip_h264_hrd(reader)?;
    }
    let vcl_hrd = reader.read_flag()?;
    if vcl_hrd {
        skip_h264_hrd(reader)?;
    }
    if nal_hrd || vcl_hrd {
        reader.read_flag()?;
    }
    reader.read_flag()?;
    Some(())
}

fn skip_h264_hrd(reader: &mut BitReader<'_>) -> Option<()> {
    let cpb_cnt_minus1 = reader.read_exp_golomb()?;
    if cpb_cnt_minus1 > 31 {
        return None;
    }
    reader.skip_bits(8)?;
    for _ in 0..=cpb_cnt_minus1 {
        reader.skip_exp_golombs(2)?;
        reader.read_flag()?;
    }
    reader.skip_bits(20)
}

fn parse_hevc_sps(nal: &[u8]) -> Option<StreamMetadata> {
    let mut reader = BitReader::new(nal.get(2..)?);
    reader.skip_bits(4)?;
    let max_sub_layers_minus1 = usize::try_from(reader.read_bits(3)?).ok()?;
    reader.read_flag()?;
    skip_hevc_profile_tier_level(&mut reader, max_sub_layers_minus1)?;
    let log2_max_poc_lsb_minus4 = skip_hevc_geometry(&mut reader)?;
    let mut meta = StreamMetadata {
        max_reorder_frames: read_hevc_reorder_pics(&mut reader, max_sub_layers_minus1)?,
        ..StreamMetadata::default()
    };
    let _: Option<()> = parse_hevc_sps_tail(&mut reader, log2_max_poc_lsb_minus4, &mut meta);
    Some(meta)
}

fn skip_hevc_geometry(reader: &mut BitReader<'_>) -> Option<u32> {
    reader.read_exp_golomb()?;
    if reader.read_exp_golomb()? == 3 {
        reader.read_flag()?;
    }
    reader.skip_exp_golombs(2)?;
    if reader.read_flag()? {
        reader.skip_exp_golombs(4)?;
    }
    reader.skip_exp_golombs(2)?;
    reader.read_exp_golomb()
}

fn skip_hevc_profile_tier_level(reader: &mut BitReader<'_>, max_sub_layers: usize) -> Option<()> {
    reader.skip_bits(96)?;
    let mut profile_present = [false; 8];
    let mut level_present = [false; 8];
    for i in 0..max_sub_layers {
        profile_present[i] = reader.read_flag()?;
        level_present[i] = reader.read_flag()?;
    }
    if max_sub_layers > 0 {
        reader.skip_bits((8 - max_sub_layers) * 2)?;
    }
    for i in 0..max_sub_layers {
        if profile_present[i] {
            reader.skip_bits(88)?;
        }
        if level_present[i] {
            reader.skip_bits(8)?;
        }
    }
    Some(())
}

fn read_hevc_reorder_pics(
    reader: &mut BitReader<'_>,
    max_sub_layers_minus1: usize,
) -> Option<Option<usize>> {
    let ordering_present = reader.read_flag()?;
    let start = if ordering_present {
        0
    } else {
        max_sub_layers_minus1
    };
    let mut max_reorder = 0_usize;
    for _ in start..=max_sub_layers_minus1 {
        reader.read_exp_golomb()?;
        let reorder = usize::try_from(reader.read_exp_golomb()?).ok()?;
        max_reorder = max_reorder.max(reorder);
        reader.read_exp_golomb()?;
    }
    Some(Some(max_reorder))
}

fn parse_hevc_sps_tail(
    reader: &mut BitReader<'_>,
    log2_max_poc_lsb_minus4: u32,
    meta: &mut StreamMetadata,
) -> Option<()> {
    reader.skip_exp_golombs(6)?;
    if reader.read_flag()? && reader.read_flag()? {
        skip_hevc_scaling_list(reader)?;
    }
    skip_hevc_pcm(reader)?;
    skip_hevc_short_term_rps(reader)?;
    skip_hevc_long_term_rps(reader, log2_max_poc_lsb_minus4)?;
    reader.skip_bits(2)?;
    if reader.read_flag()? {
        skip_vui_aspect_and_overscan(reader)?;
        parse_vui_video_signal_type(reader, meta)?;
    }
    Some(())
}

fn skip_hevc_pcm(reader: &mut BitReader<'_>) -> Option<()> {
    reader.skip_bits(2)?;
    if reader.read_flag()? {
        reader.skip_bits(8)?;
        reader.skip_exp_golombs(2)?;
        reader.read_flag()?;
    }
    Some(())
}

fn skip_hevc_long_term_rps(reader: &mut BitReader<'_>, log2_max_poc_lsb_minus4: u32) -> Option<()> {
    if reader.read_flag()? {
        let count = reader.read_exp_golomb()?;
        let poc_bits = (log2_max_poc_lsb_minus4 + 4) as usize;
        for _ in 0..count {
            reader.skip_bits(poc_bits + 1)?;
        }
    }
    Some(())
}

fn skip_hevc_scaling_list(reader: &mut BitReader<'_>) -> Option<()> {
    for size_id in 0..4_u8 {
        let step = if size_id == 3 { 3 } else { 1 };
        let mut matrix_id = 0_u8;
        while matrix_id < 6 {
            if !reader.read_flag()? {
                reader.read_exp_golomb()?;
            } else {
                let coef_num = 64_usize.min(1_usize << (4 + (size_id << 1)));
                let extra = usize::from(size_id > 1);
                reader.skip_exp_golombs(coef_num + extra)?;
            }
            matrix_id += step;
        }
    }
    Some(())
}

fn skip_hevc_short_term_rps(reader: &mut BitReader<'_>) -> Option<()> {
    let num_sets = usize::try_from(reader.read_exp_golomb()?).ok()?;
    if num_sets > 64 {
        return None;
    }
    let mut num_delta_pocs = [0_usize; 65];
    for index in 0..num_sets {
        let inter = index != 0 && reader.read_flag()?;
        num_delta_pocs[index] = if inter {
            skip_hevc_inter_rps(reader, num_delta_pocs[index - 1])?
        } else {
            skip_hevc_intra_rps(reader)?
        };
    }
    Some(())
}

fn skip_hevc_inter_rps(reader: &mut BitReader<'_>, prev_delta_pocs: usize) -> Option<usize> {
    reader.read_flag()?;
    reader.read_exp_golomb()?;
    let mut next = 0_usize;
    for _ in 0..=prev_delta_pocs {
        if reader.read_flag()? || reader.read_flag()? {
            next += 1;
        }
    }
    Some(next)
}

fn skip_hevc_intra_rps(reader: &mut BitReader<'_>) -> Option<usize> {
    let neg = usize::try_from(reader.read_exp_golomb()?).ok()?;
    let pos = usize::try_from(reader.read_exp_golomb()?).ok()?;
    for _ in 0..(neg + pos) {
        reader.read_exp_golomb()?;
        reader.read_flag()?;
    }
    Some(neg + pos)
}

fn skip_vui_aspect_and_overscan(reader: &mut BitReader<'_>) -> Option<()> {
    if reader.read_flag()? {
        let idc = reader.read_bits(8)?;
        if idc == 255 {
            reader.skip_bits(32)?;
        }
    }
    if reader.read_flag()? {
        reader.read_flag()?;
    }
    Some(())
}

fn parse_vui_video_signal_type(
    reader: &mut BitReader<'_>,
    meta: &mut StreamMetadata,
) -> Option<()> {
    if !reader.read_flag()? {
        return Some(());
    }
    reader.skip_bits(3)?;
    meta.range = Some(if reader.read_flag()? {
        ColorRange::Full
    } else {
        ColorRange::Limited
    });
    if reader.read_flag()? {
        let primaries = u8::try_from(reader.read_bits(8)?).ok()?;
        let transfer = u8::try_from(reader.read_bits(8)?).ok()?;
        let matrix = u8::try_from(reader.read_bits(8)?).ok()?;
        meta.primaries = ColorPrimaries::from_h273(primaries)
            .or_else(|| ColorPrimaries::from_matrix_h273(matrix));
        meta.transfer = ColorTransfer::from_h273(transfer);
    }
    Some(())
}

// Formats an RFC 6381 / WebCodecs codec string (`avc1.PPCCLL`, `hvc1...`, `av01...`) from an
// Annex-B SPS NAL payload (no start code), falling back to a conservative profile/level default.
#[cfg(any(
    test,
    all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
))]
pub(super) fn webcodecs_codec_string(codec: VideoCodec, sps: Option<&[u8]>) -> String {
    match codec {
        VideoCodec::H264 => {
            if let Some(nal) = sps
                && nal.len() >= 4
            {
                return format!("avc1.{:02x}{:02x}{:02x}", nal[1], nal[2], nal[3]);
            }
            "avc1.640028".into()
        }
        VideoCodec::Hevc => sps
            .and_then(hevc_codec_string_from_sps)
            .unwrap_or_else(|| "hvc1.1.6.L93.B0".into()),
        VideoCodec::Av1 => "av01.0.04M.08".into(),
    }
}

#[cfg(any(
    test,
    all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
))]
fn hevc_codec_string_from_sps(nal: &[u8]) -> Option<String> {
    let mut reader = BitReader::new(nal.get(2..)?);
    // sps_video_parameter_set_id(4) + sps_max_sub_layers_minus1(3) + temporal_id_nesting(1)
    reader.skip_bits(8)?;
    let profile_space = reader.read_bits(2)?;
    let tier_flag = reader.read_flag()?;
    let profile_idc = reader.read_bits(5)?;
    let compat_flags = reader.read_bits(32)?;
    let mut constraints = [0_u8; 6];
    for byte in &mut constraints {
        *byte = u8::try_from(reader.read_bits(8)?).ok()?;
    }
    let level_idc = reader.read_bits(8)?;
    let space_prefix = match profile_space {
        1 => "A",
        2 => "B",
        3 => "C",
        _ => "",
    };
    let tier = if tier_flag { 'H' } else { 'L' };
    let compat_rev = compat_flags.reverse_bits();
    let mut out = format!("hvc1.{space_prefix}{profile_idc}.{compat_rev:X}.{tier}{level_idc}");
    let last_nonzero = constraints.iter().rposition(|&b| b != 0).unwrap_or(0);
    for &b in &constraints[..=last_nonzero] {
        use std::fmt::Write as _;
        write!(out, ".{b:02X}").ok()?;
    }
    Some(out)
}

#[cfg(test)]
#[path = "tests/sps_tests.rs"]
mod tests;
