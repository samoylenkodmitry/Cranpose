//! ETC1S codebook decode and slice transcode. Decompresses the endpoint +
//! selector codebooks and the slice-decode Huffman models using the BasisLZ
//! entropy codec, then decodes slices into per-block indices and emits them in
//! each supported output format.

use super::amf::ApproxMoveToFront;
use super::decoder::BitwiseDecoder;
use super::huffman::HuffmanDecodingTable;
use crate::color::Color32;
use crate::etc::block::DecoderEtcBlock;
use crate::etc::tables::SELECTOR_INDEX_TO_ETC1;
use alloc::vec::Vec;

/// Highest previous-component value decoded with endpoint delta model 0.
const COLOR5_PAL0_PREV_HI: i32 = 9;
/// Highest previous-component value decoded with endpoint delta model 1;
/// anything larger uses model 2.
const COLOR5_PAL1_PREV_HI: i32 = 21;
/// Rounded rescale of an 8-bit `v` to `0..=q`: computes `round(v * q / 255)`
/// without a divide (the `+128` plus the `>> 8` feedback term make the shift
/// an exact rounding division by 255). Used by the packed 565/4444 emits.
#[inline]
fn mul_8(v: u32, q: u32) -> u32 {
    let v = v * q + 128;
    (v + (v >> 8)) >> 8
}

/// Last symbol (256) of the endpoint-prediction alphabet: the four 2-bit
/// predictors of a 2x2 block group fill symbols 0..=255, and this extra symbol
/// signals a repeat run of the previous group's predictors.
const ENDPOINT_PRED_REPEAT_LAST_SYMBOL: u32 = (4 * 4 * 4 * 4 + 1) - 1;
/// Shortest repeat run the encoder emits; decoded run lengths are biased by it.
const ENDPOINT_PRED_MIN_REPEAT_COUNT: u32 = 3;
/// Chunk size of the variable-length repeat count that follows the repeat symbol.
const ENDPOINT_PRED_COUNT_VLC_BITS: u32 = 4;
/// Shortest selector-history RLE run; decoded run lengths are biased by it.
const SELECTOR_HISTORY_BUF_RLE_COUNT_THRESH: u32 = 3;
/// Size (64) of the selector-history RLE run-length alphabet; the last symbol
/// escapes to a VLC-coded count for longer runs.
const SELECTOR_HISTORY_BUF_RLE_COUNT_TOTAL: u32 = 1 << 6;

/// ETC1S endpoint: 5-bit base color + 3-bit intensity.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Endpoint {
    pub color5: Color32,
    pub inten5: u8,
}

/// ETC1S selector block: the 2-bit-per-texel selectors plus the ETC1-format
/// bytes, with cached histogram flags.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Selector {
    pub selectors: [u8; 4],
    pub bytes: [u8; 4],
    pub lo_selector: u8,
    pub hi_selector: u8,
    pub num_unique_selectors: u8,
}

impl Selector {
    /// The 2-bit selector at texel (`x`, `y`), where each `selectors[y]` byte
    /// packs one row of four texels low-bit-first.
    pub fn get_selector(&self, x: u32, y: u32) -> u32 {
        ((self.selectors[y as usize] >> (x * 2)) & 3) as u32
    }

    /// Set the 2-bit selector at texel (`x`, `y`) and keep the ETC1-format
    /// `bytes` in sync. ETC1 splits each selector into an LSB plane and an MSB
    /// plane stored in separate bytes (the `pi` / `pi - 2` writes), indexed by
    /// the ETC1 texel order `x*4 + y`, after remapping the value through
    /// `SELECTOR_INDEX_TO_ETC1`.
    pub fn set_selector(&mut self, x: u32, y: u32, val: u32) {
        self.selectors[y as usize] &= !(3 << (x * 2));
        self.selectors[y as usize] |= (val << (x * 2)) as u8;

        let etc1_bit_index = x * 4 + y;
        let pi = (3 - (etc1_bit_index >> 3)) as usize;
        let byte_bit_ofs = etc1_bit_index & 7;
        let mask = 1u8 << byte_bit_ofs;
        let etc1_val = SELECTOR_INDEX_TO_ETC1[val as usize] as u32;
        let lsb = (etc1_val & 1) as u8;
        let msb = (etc1_val >> 1) as u8;

        self.bytes[pi] &= !mask;
        self.bytes[pi] |= lsb << byte_bit_ofs;
        self.bytes[pi - 2] &= !mask;
        self.bytes[pi - 2] |= msb << byte_bit_ofs;
    }

    /// Recompute the cached selector histogram flags (`lo_selector`,
    /// `hi_selector`, `num_unique_selectors`) from the 16 texel selectors. The
    /// format emitters use these to pick single-color and 2-value fast paths.
    pub fn init_flags(&mut self) {
        let mut hist = [0u32; 4];
        for y in 0..4 {
            for x in 0..4 {
                hist[self.get_selector(x, y) as usize] += 1;
            }
        }
        self.lo_selector = 3;
        self.hi_selector = 0;
        self.num_unique_selectors = 0;
        for i in 0..4u8 {
            if hist[i as usize] != 0 {
                self.num_unique_selectors += 1;
                if i < self.lo_selector {
                    self.lo_selector = i;
                }
                if i > self.hi_selector {
                    self.hi_selector = i;
                }
            }
        }
    }
}

/// Pack a decoded ETC1S endpoint + selector pair into an 8-byte ETC1 block
/// (also used as the color half of an ETC2_RGBA block). ETC1S is a strict
/// subset of ETC1: differential mode with a zero delta, one intensity table
/// for both subblocks.
pub fn convert_etc1s_to_etc1(ep: &Endpoint, sel: &Selector) -> [u8; 8] {
    let mut blk = DecoderEtcBlock::new();
    blk.set_flip_bit(false);
    blk.set_diff_bit(true);
    blk.set_base5_color(DecoderEtcBlock::pack_color5_unscaled(ep.color5));
    blk.set_inten_table(0, ep.inten5 as u32);
    blk.set_inten_table(1, ep.inten5 as u32);
    let mut out = [0u8; 8];
    out[0..4].copy_from_slice(&blk.m_bytes[0..4]);
    out[4..8].copy_from_slice(&sel.bytes);
    out
}

/// Maximum number of mip levels a video keeps previous-frame state for.
pub const MAX_PREV_FRAME_LEVELS: usize = 16;

/// Cross-frame decode state for ETC1S video.
///
/// A video P-frame stores only the blocks that changed; an unchanged block is
/// coded as a conditional-replenishment prediction that reuses the previous
/// frame's `(endpoint, selector)` pair for the same block position. This holds
/// that pair per block, packed `endpoint_index | selector_index << 16`, one
/// array per (alpha slice, mip level). Frames must be decoded in order with
/// the same state for the output to be meaningful.
#[derive(Default)]
pub struct VideoState {
    /// `[alpha_flag][level_index]` -> previous frame's per-block indices.
    prev_frame_indices: [[Vec<u32>; MAX_PREV_FRAME_LEVELS]; 2],
}

impl VideoState {
    /// Fresh state: decode the next frame as if it were the first.
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop all previous-frame data, e.g. before seeking back to frame 0.
    pub fn reset(&mut self) {
        for per_level in &mut self.prev_frame_indices {
            for v in per_level {
                v.clear();
            }
        }
    }

    /// The color and alpha prev-frame arrays for one mip level, each grown to
    /// `total_blocks`. Newly grown entries are zero, so a block with no prior
    /// history reads as endpoint 0 / selector 0.
    pub(crate) fn slots(
        &mut self,
        level: usize,
        total_blocks: usize,
    ) -> (&mut Vec<u32>, &mut Vec<u32>) {
        let [color, alpha] = &mut self.prev_frame_indices;
        let (c, a) = (&mut color[level], &mut alpha[level]);
        if c.len() < total_blocks {
            c.resize(total_blocks, 0);
        }
        if a.len() < total_blocks {
            a.resize(total_blocks, 0);
        }
        (c, a)
    }
}

/// The decoded ETC1S codebooks + slice-decode models.
#[derive(Default)]
pub struct Etc1sTranscoder {
    /// Decoded endpoint codebook.
    pub endpoints: Vec<Endpoint>,
    /// Decoded selector codebook.
    pub selectors: Vec<Selector>,
    /// Model for the per-2x2-group endpoint predictor symbols.
    pub endpoint_pred_model: HuffmanDecodingTable,
    /// Model for endpoint index deltas.
    pub delta_endpoint_model: HuffmanDecodingTable,
    /// Model for selector symbols (codebook indices, history references, and
    /// the RLE escape).
    pub selector_model: HuffmanDecodingTable,
    /// Model for selector-history RLE run lengths.
    pub selector_history_buf_rle_model: HuffmanDecodingTable,
    /// Capacity of the selector history buffer used during slice decode.
    pub selector_history_buf_size: u32,
}

impl Etc1sTranscoder {
    /// An empty transcoder. Call [`Self::decode_palettes`] and
    /// [`Self::decode_tables`] to populate the codebooks and slice models before
    /// transcoding.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a transcoder from an externally supplied endpoint + selector
    /// codebook, then decode this file's own Huffman slice-decode tables.
    ///
    /// This is the global-codebook path: a `.basis` file can reference a
    /// codebook stored in a separate file instead of carrying its own palettes,
    /// so the shared `endpoints`/`selectors` stand in for what [`decode_palettes`]
    /// would otherwise decode, while the per-file `tables` still supply the
    /// slice-decode models. `None` if the tables fail to decode.
    ///
    /// [`decode_palettes`]: Self::decode_palettes
    pub fn with_global_codebook(
        endpoints: Vec<Endpoint>,
        selectors: Vec<Selector>,
        tables: &[u8],
    ) -> Option<Self> {
        let mut dec = Self {
            endpoints,
            selectors,
            ..Self::default()
        };
        dec.decode_tables(tables).then_some(dec)
    }

    /// Decompress the endpoint and selector codebooks from their two
    /// compressed streams.
    pub fn decode_palettes(
        &mut self,
        num_endpoints: u32,
        endpoints_data: &[u8],
        num_selectors: u32,
        selectors_data: &[u8],
    ) -> bool {
        let mut sym = BitwiseDecoder::new(endpoints_data);
        let (mut m0, mut m1, mut m2, mut im) = (
            HuffmanDecodingTable::new(),
            HuffmanDecodingTable::new(),
            HuffmanDecodingTable::new(),
            HuffmanDecodingTable::new(),
        );
        if !sym.read_huffman_table(&mut m0)
            || !sym.read_huffman_table(&mut m1)
            || !sym.read_huffman_table(&mut m2)
            || !sym.read_huffman_table(&mut im)
        {
            return false;
        }
        if !m0.is_valid() || !m1.is_valid() || !m2.is_valid() || !im.is_valid() {
            return false;
        }

        let grayscale = sym.get_bits(1) != 0;
        self.endpoints = vec![Endpoint::default(); num_endpoints as usize];
        let mut prev_color5 = Color32::new(16, 16, 16, 0);
        let mut prev_inten = 0u32;

        for e in self.endpoints.iter_mut() {
            let inten_delta = sym.decode_huffman(&im);
            e.inten5 = ((inten_delta + prev_inten) & 7) as u8;
            prev_inten = e.inten5 as u32;

            let nc = if grayscale { 1 } else { 3 };
            for c in 0..nc {
                let prev = prev_color5[c] as i32;
                let delta = if prev <= COLOR5_PAL0_PREV_HI {
                    sym.decode_huffman(&m0)
                } else if prev <= COLOR5_PAL1_PREV_HI {
                    sym.decode_huffman(&m1)
                } else {
                    sym.decode_huffman(&m2)
                };
                let v = ((prev + delta as i32) & 31) as u8;
                e.color5[c] = v;
                prev_color5[c] = v;
            }
            if grayscale {
                e.color5[1] = e.color5[0];
                e.color5[2] = e.color5[0];
            }
        }

        // selector codebook stream
        let mut sym = BitwiseDecoder::new(selectors_data);
        self.selectors = vec![Selector::default(); num_selectors as usize];

        if sym.get_bits(1) == 1 {
            return false; // global selector codebooks unsupported
        }
        if sym.get_bits(1) == 1 {
            return false; // hybrid selector codebooks unsupported
        }
        let used_raw = sym.get_bits(1) == 1;

        if used_raw {
            for s in self.selectors.iter_mut() {
                for j in 0..4u32 {
                    let cur = sym.get_bits(8);
                    for k in 0..4u32 {
                        s.set_selector(k, j, (cur >> (k * 2)) & 3);
                    }
                }
                s.init_flags();
            }
        } else {
            let mut dm = HuffmanDecodingTable::new();
            if !sym.read_huffman_table(&mut dm) {
                return false;
            }
            if num_selectors > 1 && !dm.is_valid() {
                return false;
            }
            let mut prev_bytes = [0u8; 4];
            for (i, s) in self.selectors.iter_mut().enumerate() {
                if i == 0 {
                    for j in 0..4u32 {
                        let cur = sym.get_bits(8);
                        prev_bytes[j as usize] = cur as u8;
                        for k in 0..4u32 {
                            s.set_selector(k, j, (cur >> (k * 2)) & 3);
                        }
                    }
                    s.init_flags();
                    continue;
                }
                for j in 0..4u32 {
                    let delta_byte = sym.decode_huffman(&dm);
                    let cur = delta_byte ^ prev_bytes[j as usize] as u32;
                    prev_bytes[j as usize] = cur as u8;
                    for k in 0..4u32 {
                        s.set_selector(k, j, (cur >> (k * 2)) & 3);
                    }
                }
                s.init_flags();
            }
        }

        true
    }

    /// Decode a slice into per-block `(endpoint_index, selector_index)` pairs in
    /// block order (`block_x + block_y * num_blocks_x`). This is the
    /// format-independent core (endpoint-prediction + selector-history state
    /// machine); each emit target formats these. `None` on a corrupt stream.
    ///
    /// `video` carries the previous frame's per-block indices for this slice
    /// when decoding a video frame: predictor 2 then means conditional
    /// replenishment (reuse that block's previous pair, no selector symbol in
    /// the stream), and every decoded block is written back for the next
    /// frame. `None` decodes a still image, where predictor 2 is the
    /// upper-left neighbor.
    pub(crate) fn decode_slice_indices(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        mut video: Option<&mut Vec<u32>>,
    ) -> Option<Vec<(u16, u16)>> {
        let total_blocks = num_blocks_x * num_blocks_y;
        if self.endpoints.is_empty() || self.selectors.is_empty() {
            return None;
        }
        let n_end = self.endpoints.len() as u32;
        let n_sel = self.selectors.len() as u32;

        let mut sym = BitwiseDecoder::new(slice_data);
        let mut selector_history_buf =
            ApproxMoveToFront::new(self.selector_history_buf_size as usize);
        let mut cur_selector_rle_count = 0u32;

        // Per-row endpoint-prediction state: (pred_bits, endpoint_index).
        let mut preds: [Vec<(u8, u16)>; 2] = [
            vec![(0u8, 0u16); num_blocks_x as usize],
            vec![(0u8, 0u16); num_blocks_x as usize],
        ];

        let mut cur_pred_bits = 0u32;
        let mut prev_endpoint_pred_sym = 0u32;
        let mut endpoint_pred_repeat_count = 0u32;
        let mut prev_endpoint_index = 0u32;

        let selector_rle_sym = self.selector_history_buf_size + n_sel;

        let mut indices = vec![(0u16, 0u16); total_blocks as usize];

        for block_y in 0..num_blocks_y {
            let cur = (block_y & 1) as usize;
            for block_x in 0..num_blocks_x {
                let bx = block_x as usize;

                // Decode endpoint index predictor symbols.
                if block_x & 1 == 0 {
                    if block_y & 1 == 0 {
                        if endpoint_pred_repeat_count != 0 {
                            endpoint_pred_repeat_count -= 1;
                            cur_pred_bits = prev_endpoint_pred_sym;
                        } else {
                            cur_pred_bits = sym.decode_huffman(&self.endpoint_pred_model);
                            if cur_pred_bits == ENDPOINT_PRED_REPEAT_LAST_SYMBOL {
                                endpoint_pred_repeat_count = sym
                                    .decode_vlc(ENDPOINT_PRED_COUNT_VLC_BITS)
                                    + ENDPOINT_PRED_MIN_REPEAT_COUNT
                                    - 1;
                                cur_pred_bits = prev_endpoint_pred_sym;
                            } else {
                                prev_endpoint_pred_sym = cur_pred_bits;
                            }
                        }
                        preds[cur ^ 1][bx].0 = (cur_pred_bits >> 4) as u8;
                    } else {
                        cur_pred_bits = preds[cur][bx].0 as u32;
                    }
                }

                let pred = cur_pred_bits & 3;
                cur_pred_bits >>= 2;

                // For a video conditional-replenishment block the selector
                // came from the previous frame along with the endpoint, and no
                // selector symbol is present in the stream.
                let mut cr_selector: Option<u32> = None;

                let endpoint_index = match pred {
                    0 => {
                        if block_x == 0 {
                            return None;
                        }
                        prev_endpoint_index
                    }
                    1 => {
                        if block_y == 0 {
                            return None;
                        }
                        preds[cur ^ 1][bx].1 as u32
                    }
                    2 => {
                        if let Some(prev) = video.as_deref_mut() {
                            let v = prev[(block_x + block_y * num_blocks_x) as usize];
                            cr_selector = Some(v >> 16);
                            v & 0xFFFF
                        } else {
                            if block_x == 0 || block_y == 0 {
                                return None;
                            }
                            preds[cur ^ 1][bx - 1].1 as u32
                        }
                    }
                    _ => {
                        let delta = sym.decode_huffman(&self.delta_endpoint_model);
                        let mut ei = delta + prev_endpoint_index;
                        if ei >= n_end {
                            ei -= n_end;
                        }
                        ei
                    }
                };
                preds[cur][bx].1 = endpoint_index as u16;
                prev_endpoint_index = endpoint_index;

                // Decode selector index (absent for a replenished video block).
                let selector_index = if let Some(si) = cr_selector {
                    si
                } else {
                    let mut selector_sym: u32;
                    if cur_selector_rle_count > 0 {
                        cur_selector_rle_count -= 1;
                        selector_sym = n_sel;
                    } else {
                        selector_sym = sym.decode_huffman(&self.selector_model);
                        if selector_sym == selector_rle_sym {
                            let run_sym = sym.decode_huffman(&self.selector_history_buf_rle_model);
                            cur_selector_rle_count =
                                if run_sym == SELECTOR_HISTORY_BUF_RLE_COUNT_TOTAL - 1 {
                                    sym.decode_vlc(7) + SELECTOR_HISTORY_BUF_RLE_COUNT_THRESH
                                } else {
                                    run_sym + SELECTOR_HISTORY_BUF_RLE_COUNT_THRESH
                                };
                            if cur_selector_rle_count > total_blocks {
                                return None;
                            }
                            selector_sym = n_sel;
                            cur_selector_rle_count -= 1;
                        }
                    }

                    if selector_sym >= n_sel {
                        let hbi = (selector_sym - n_sel) as usize;
                        if hbi >= selector_history_buf.size() {
                            return None;
                        }
                        let si = selector_history_buf.get(hbi) as u32;
                        if hbi != 0 {
                            selector_history_buf.use_index(hbi);
                        }
                        si
                    } else {
                        if self.selector_history_buf_size != 0 {
                            selector_history_buf.add(selector_sym as i32);
                        }
                        selector_sym
                    }
                };

                if endpoint_index >= n_end || selector_index >= n_sel {
                    return None;
                }

                if let Some(prev) = video.as_deref_mut() {
                    prev[(block_x + block_y * num_blocks_x) as usize] =
                        endpoint_index | (selector_index << 16);
                }

                indices[(block_x + block_y * num_blocks_x) as usize] =
                    (endpoint_index as u16, selector_index as u16);
            }
        }

        Some(indices)
    }

    /// Transcode a slice to 8-byte ETC1 blocks, written into `out` (exactly
    /// `num_blocks_x*num_blocks_y*8` bytes; also used for the color half
    /// of ETC2_RGBA output).
    pub fn transcode_slice_etc1(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != indices.len() * 8 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block =
                convert_etc1s_to_etc1(&self.endpoints[ei as usize], &self.selectors[si as usize]);
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }

    /// Transcode a slice to PVRTC1 4bpp RGB. PVRTC1 blends endpoints across
    /// block boundaries, so this decodes the slice indices and then drives the
    /// whole-image prep + apply pass in `crate::pvrtc`. `None` for non-pow2
    /// dimensions, which PVRTC1 cannot represent.
    pub fn transcode_slice_pvrtc1_4_rgb(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        crate::pvrtc::transcode_etc1s_pvrtc1_4_rgb(
            &indices,
            &self.endpoints,
            &self.selectors,
            num_blocks_x,
            num_blocks_y,
            out,
        )
    }

    /// Transcode color + alpha slices to PVRTC1 4bpp RGBA. Decodes both the
    /// color (`rgb_slice`) and alpha (`alpha_slice`) per-block index streams,
    /// then drives the whole-image RGBA prep + apply pass in `crate::pvrtc`.
    /// `video`, if present, is the previous frame's (color, alpha) index slots
    /// for ETC1S video. `None` for non-pow2 dimensions.
    pub fn transcode_slice_pvrtc1_4_rgba(
        &self,
        rgb_slice: &[u8],
        alpha_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<(&mut Vec<u32>, &mut Vec<u32>)>,
        out: &mut [u8],
    ) -> Option<()> {
        let (vc, va) = match video {
            Some((c, a)) => (Some(c), Some(a)),
            None => (None, None),
        };
        let color = self.decode_slice_indices(rgb_slice, num_blocks_x, num_blocks_y, vc)?;
        let alpha = self.decode_slice_indices(alpha_slice, num_blocks_x, num_blocks_y, va)?;
        crate::pvrtc::transcode_etc1s_pvrtc1_4_rgba(
            &color,
            &alpha,
            &self.endpoints,
            &self.selectors,
            num_blocks_x,
            num_blocks_y,
            out,
        )
    }

    /// Transcode a slice to RGBA32, written into `out` (exactly
    /// `width * height * 4` bytes, row pitch = width): each block's four
    /// candidate colors are indexed by its per-texel selectors. Alpha is 255.
    #[allow(clippy::too_many_arguments)]
    pub fn transcode_slice_rgba32(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        width: u32,
        height: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != (width * height * 4) as usize {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block_x = b as u32 % num_blocks_x;
            let block_y = b as u32 / num_blocks_x;
            let ep = &self.endpoints[ei as usize];
            let sel = &self.selectors[si as usize];
            let colors = DecoderEtcBlock::get_block_colors5(ep.color5, ep.inten5 as u32);

            let max_x = (width as i32 - block_x as i32 * 4).clamp(0, 4) as u32;
            let max_y = (height as i32 - block_y as i32 * 4).clamp(0, 4) as u32;
            for y in 0..max_y {
                let s = sel.selectors[y as usize] as u32;
                for x in 0..max_x {
                    let c = colors[((s >> (x * 2)) & 3) as usize];
                    let px = ((block_x * 4 + x) + (block_y * 4 + y) * width) as usize * 4;
                    out[px] = c.r();
                    out[px + 1] = c.g();
                    out[px + 2] = c.b();
                    out[px + 3] = 255;
                }
            }
        }
        Some(())
    }

    /// ETC1S with alpha to RGBA32: the RGB pass (above) then an alpha pass
    /// that overwrites the A channel with the alpha slice's grayscale (green
    /// channel) values. `video`, if present, is the previous frame's
    /// (color, alpha) index slots for ETC1S video.
    #[allow(clippy::too_many_arguments)]
    pub fn transcode_image_rgba32(
        &self,
        rgb_slice: &[u8],
        alpha_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        width: u32,
        height: u32,
        video: Option<(&mut Vec<u32>, &mut Vec<u32>)>,
        out: &mut [u8],
    ) -> Option<()> {
        let (vc, va) = match video {
            Some((c, a)) => (Some(c), Some(a)),
            None => (None, None),
        };
        self.transcode_slice_rgba32(
            rgb_slice,
            num_blocks_x,
            num_blocks_y,
            width,
            height,
            vc,
            out,
        )?;
        let alpha = self.decode_slice_indices(alpha_slice, num_blocks_x, num_blocks_y, va)?;
        for (b, &(aei, asi)) in alpha.iter().enumerate() {
            let block_x = b as u32 % num_blocks_x;
            let block_y = b as u32 / num_blocks_x;
            let ep = &self.endpoints[aei as usize];
            let sel = &self.selectors[asi as usize];
            let acolors = DecoderEtcBlock::get_block_colors5_g(ep.color5, ep.inten5 as u32);
            let max_x = (width as i32 - block_x as i32 * 4).clamp(0, 4) as u32;
            let max_y = (height as i32 - block_y as i32 * 4).clamp(0, 4) as u32;
            for y in 0..max_y {
                let s = sel.selectors[y as usize] as u32;
                for x in 0..max_x {
                    let a = acolors[((s >> (x * 2)) & 3) as usize] as u8;
                    let px = ((block_x * 4 + x) + (block_y * 4 + y) * width) as usize * 4;
                    out[px + 3] = a;
                }
            }
        }
        Some(())
    }

    /// Transcode a slice to 2-byte/pixel 565 in raster order, written into
    /// `out` (exactly `width * height * 2` bytes). `bgr` packs B
    /// into the high bits (R at bit 0) instead of R at bit 11. `video`, if
    /// present, is the previous frame's per-block indices for this slice
    /// (ETC1S video).
    #[allow(clippy::too_many_arguments)]
    pub fn transcode_slice_565(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        width: u32,
        height: u32,
        bgr: bool,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != (width * height * 2) as usize {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block_x = b as u32 % num_blocks_x;
            let block_y = b as u32 / num_blocks_x;
            let ep = &self.endpoints[ei as usize];
            let sel = &self.selectors[si as usize];
            let colors = DecoderEtcBlock::get_block_colors5(ep.color5, ep.inten5 as u32);

            let mut packed = [0u16; 4];
            for (i, c) in colors.iter().enumerate() {
                packed[i] = if bgr {
                    ((mul_8(c.b() as u32, 31) << 11)
                        | (mul_8(c.g() as u32, 63) << 5)
                        | mul_8(c.r() as u32, 31)) as u16
                } else {
                    ((mul_8(c.r() as u32, 31) << 11)
                        | (mul_8(c.g() as u32, 63) << 5)
                        | mul_8(c.b() as u32, 31)) as u16
                };
            }

            let max_x = (width as i32 - block_x as i32 * 4).clamp(0, 4) as u32;
            let max_y = (height as i32 - block_y as i32 * 4).clamp(0, 4) as u32;
            for y in 0..max_y {
                let s = sel.selectors[y as usize] as u32;
                for x in 0..max_x {
                    let v = packed[((s >> (x * 2)) & 3) as usize];
                    let px = ((block_x * 4 + x) + (block_y * 4 + y) * width) as usize * 2;
                    out[px..px + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        Some(())
    }

    /// Transcode a slice to RGBA4444 with alpha forced to 0xF, written into
    /// `out` (exactly `width * height * 2` bytes): RGB packs into
    /// the high 12 bits (R at bit 12, A at bit 0). Used when the ETC1S file
    /// carries no alpha slice.
    #[allow(clippy::too_many_arguments)]
    pub fn transcode_slice_rgba4444_opaque(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        width: u32,
        height: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != (width * height * 2) as usize {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block_x = b as u32 % num_blocks_x;
            let block_y = b as u32 / num_blocks_x;
            let ep = &self.endpoints[ei as usize];
            let sel = &self.selectors[si as usize];
            let colors = DecoderEtcBlock::get_block_colors5(ep.color5, ep.inten5 as u32);

            let mut packed = [0u16; 4];
            for (i, c) in colors.iter().enumerate() {
                packed[i] = ((mul_8(c.r() as u32, 15) << 12)
                    | (mul_8(c.g() as u32, 15) << 8)
                    | (mul_8(c.b() as u32, 15) << 4)
                    | 0xF) as u16;
            }

            let max_x = (width as i32 - block_x as i32 * 4).clamp(0, 4) as u32;
            let max_y = (height as i32 - block_y as i32 * 4).clamp(0, 4) as u32;
            for y in 0..max_y {
                let s = sel.selectors[y as usize] as u32;
                for x in 0..max_x {
                    let v = packed[((s >> (x * 2)) & 3) as usize];
                    let px = ((block_x * 4 + x) + (block_y * 4 + y) * width) as usize * 2;
                    out[px..px + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        Some(())
    }

    /// ETC1S with alpha to RGBA4444, written into `out` (exactly
    /// `width * height * 2` bytes), in two passes: the alpha pass writes
    /// `mul_8(g, 15)` from the alpha slice into the low nibble of each word
    /// (zeroing the rest), then the color pass ORs RGB into the high 12 bits
    /// while preserving that nibble. `video`, if present, is the previous
    /// frame's (color, alpha) index slots for ETC1S video.
    #[allow(clippy::too_many_arguments)]
    pub fn transcode_image_rgba4444(
        &self,
        rgb_slice: &[u8],
        alpha_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        width: u32,
        height: u32,
        video: Option<(&mut Vec<u32>, &mut Vec<u32>)>,
        out: &mut [u8],
    ) -> Option<()> {
        let (vc, va) = match video {
            Some((c, a)) => (Some(c), Some(a)),
            None => (None, None),
        };
        if out.len() != (width * height * 2) as usize {
            return None;
        }
        out.fill(0);

        // alpha pass (first): low nibble = mul_8(g, 15), rest of the word 0
        let alpha = self.decode_slice_indices(alpha_slice, num_blocks_x, num_blocks_y, va)?;
        for (b, &(aei, asi)) in alpha.iter().enumerate() {
            let block_x = b as u32 % num_blocks_x;
            let block_y = b as u32 / num_blocks_x;
            let ep = &self.endpoints[aei as usize];
            let sel = &self.selectors[asi as usize];
            let acolors = DecoderEtcBlock::get_block_colors5_g(ep.color5, ep.inten5 as u32);

            let mut packed = [0u16; 4];
            for (i, &c) in acolors.iter().enumerate() {
                packed[i] = mul_8(c as u32, 15) as u16;
            }

            let max_x = (width as i32 - block_x as i32 * 4).clamp(0, 4) as u32;
            let max_y = (height as i32 - block_y as i32 * 4).clamp(0, 4) as u32;
            for y in 0..max_y {
                let s = sel.selectors[y as usize] as u32;
                for x in 0..max_x {
                    let px = ((block_x * 4 + x) + (block_y * 4 + y) * width) as usize * 2;
                    out[px..px + 2]
                        .copy_from_slice(&packed[((s >> (x * 2)) & 3) as usize].to_le_bytes());
                }
            }
        }

        // color pass (second): RGB into the high 12 bits, keeping the low nibble
        let indices = self.decode_slice_indices(rgb_slice, num_blocks_x, num_blocks_y, vc)?;
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block_x = b as u32 % num_blocks_x;
            let block_y = b as u32 / num_blocks_x;
            let ep = &self.endpoints[ei as usize];
            let sel = &self.selectors[si as usize];
            let colors = DecoderEtcBlock::get_block_colors5(ep.color5, ep.inten5 as u32);

            let mut packed = [0u16; 4];
            for (i, c) in colors.iter().enumerate() {
                packed[i] = ((mul_8(c.r() as u32, 15) << 12)
                    | (mul_8(c.g() as u32, 15) << 8)
                    | (mul_8(c.b() as u32, 15) << 4)) as u16;
            }

            let max_x = (width as i32 - block_x as i32 * 4).clamp(0, 4) as u32;
            let max_y = (height as i32 - block_y as i32 * 4).clamp(0, 4) as u32;
            for y in 0..max_y {
                let s = sel.selectors[y as usize] as u32;
                for x in 0..max_x {
                    let px = ((block_x * 4 + x) + (block_y * 4 + y) * width) as usize * 2;
                    let cur = u16::from_le_bytes([out[px], out[px + 1]]);
                    let v = (cur & 0xF) | packed[((s >> (x * 2)) & 3) as usize];
                    out[px..px + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        Some(())
    }

    /// Read the four slice-decode Huffman models plus the selector history
    /// buffer size.
    pub fn decode_tables(&mut self, table_data: &[u8]) -> bool {
        let mut sym = BitwiseDecoder::new(table_data);
        if !sym.read_huffman_table(&mut self.endpoint_pred_model)
            || self.endpoint_pred_model.code_sizes.is_empty()
        {
            return false;
        }
        if !sym.read_huffman_table(&mut self.delta_endpoint_model)
            || self.delta_endpoint_model.code_sizes.is_empty()
        {
            return false;
        }
        if !sym.read_huffman_table(&mut self.selector_model)
            || self.selector_model.code_sizes.is_empty()
        {
            return false;
        }
        if !sym.read_huffman_table(&mut self.selector_history_buf_rle_model)
            || self.selector_history_buf_rle_model.code_sizes.is_empty()
        {
            return false;
        }
        self.selector_history_buf_size = sym.get_bits(13);
        self.selector_history_buf_size != 0
    }
}
