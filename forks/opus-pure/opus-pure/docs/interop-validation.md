# Interoperability validation

This records how the crate was checked against an independent Opus implementation, what passed, and what did not. It is a snapshot, not a test that runs in CI: the reference tools are not a build dependency.

**Reference used:** `opus-tools` 0.2 (`opusinfo`, `opusdec`, `opusenc`) built against **libopus 1.6.1** — Xiph's own implementation, sharing no code with this crate.

Both directions were exercised:

- **A. Our output into their decoder.** Files written by `OpusEncoder` + `OggOpusWriter`, structurally checked with `opusinfo`, decoded with `opusdec`, then compared sample-for-sample against our own decoder on the same bitstream.
- **B. Their output into our decoder.** Files written by `opusenc`, read with `OggOpusReader` + `OpusDecoder`, compared against `opusdec` on the same file.

Both decoders ran at 48 kHz with pre-skip and end-trimming applied identically, so the outputs are sample-aligned.

## Results

### Container

`opusinfo` reports no warnings or errors on every file produced here: 13 in the feature matrix plus the 440 in the configuration sweeps below. That covers the header pages, page CRCs, sequence numbers, lacing, granule positions, and the mapping-family-1 header for 5.1 (`Streams: 4, Coupled: 2, Map: [0, 4, 1, 2, 3, 5]`).

`opusdec` decoded all of them without complaint.

### Codec, direction A

Across a 280-configuration sweep at 20 ms — every sample rate, mono and stereo, every forced bandwidth, both applications, four bitrates, speech and music:

| | |
| --- | --- |
| configurations that decode bit-identically to libopus | **40** |
| configurations agreeing to better than 100 dB SNR | **273** |
| configurations below 100 dB | 7, at 90–100 dB, every one a stream that switches coding mode |
| streams differing anywhere other than at a mode switch | **0** |
| widest window of differing samples | 3.96 ms, inside the 5 ms cross-fade |
| worst decoded peak (sources peak at 0.4–0.95) | 2.32, i.e. ordinary codec ringing |

**Bit-identical means SILK.** The 40 configurations that match to the bit are exactly the 40 whose streams are pure SILK, and the report asserts the two sets are the same rather than trusting the count. SILK is fixed-point on both sides, so it either agrees exactly or something is wrong; CELT and hybrid are float and land at float32 rounding instead (`max abs` ~2e-7). The count is therefore a check on the fixed-point path and a census of how many streams chose SILK, not a quality score.

That count used to read 168, and the fall is not a regression. The 168 were precisely the 168 configurations at 8, 12 and 16 kHz, bit-identical because this crate pinned every rate below 24 kHz to SILK — see [The 16 kHz mode decision](#the-16-khz-mode-decision). With that gate gone, 128 of them choose CELT as libopus does, which takes them off the fixed-point path. Agreement itself barely moved: 273 of 280 clear 100 dB against 270 before, on a matrix where 240 streams are now float rather than 112.

**Every differing sample sits at a mode switch.** 26 of the 280 streams change coding mode mid-file. For each stream the report locates every sample differing by more than 1e-5 and checks it against that stream's own switch points: none differs anywhere else, and the widest such window is 3.96 ms, inside the 5 ms cross-fade libopus performs there. What differs is the concealed audio the cross-fade is built from — see [The mode-switch seam](#the-mode-switch-seam). The other 19 switching streams clear 100 dB regardless.

Everything else sits at float32 rounding, which is the expected result for two independent implementations of the same transform.

The 5.1 surround case matching at 135.76 dB confirms the multistream channel mapping and self-delimited framing against a third-party decoder, not just against ourselves.

### Codec, direction A at 60 ms

60 ms is swept separately: 160 configurations — five sample rates, mono and stereo, four bandwidth settings (auto, narrowband, mediumband, wideband), both applications, 16 and 48 kb/s.

| | |
| --- | --- |
| `opusinfo` warnings or errors | **0** |
| configurations that decode bit-identically to libopus | **63** |
| configurations agreeing to better than 100 dB SNR | **160**, all of them |
| configurations below 100 dB | **0** |
| streams differing anywhere other than at a mode switch | **0** |
| widest window of differing samples | 0.00 ms — no sample in any stream differs by more than 1e-5 |

Every packet is checked to carry 60 ms before the file is written, so this is 60 ms rather than something shorter mislabelled.

As above, the 63 bit-identical streams are exactly the 63 pure-SILK ones. This table read **160 of 160** when it was first measured, for a structural reason that has since gone away: the encoder then forced *every* duration above 20 ms to SILK, so all 160 streams were fixed-point on both sides. It now chooses the mode freely and frames the result to suit — 60 ms of music comes back as three 20 ms CELT frames behind one TOC byte — so 97 of the 160 are float and agree at float rounding instead. See [Multi-frame packets](#multi-frame-packets).

The reverse direction is weaker here than it looks. `opusenc --framesize 60` reaches 60 ms by packing shorter CELT frames into a code-3 packet — its files carry configs 19, 23, 27 and 31, never the SILK 60 ms configs — so it validates our multi-frame *decoding* (six files, 137–141 dB, `max abs` 2.4e-7) rather than our reading of a SILK 60 ms packet. That path is covered instead by our own round trip and by the bit-identical direction-A result above.

### Multi-frame packets

Opus reaches durations no single frame can code by packing several frames behind one TOC byte (RFC 6716 §3.2), and libopus decides the split in `opus_encode_native`. This crate did not: it forced every duration above 20 ms to SILK — which has 40 and 60 ms configurations — and rejected 80, 100 and 120 ms outright. That was a quality cost as much as a missing feature, because 60 ms of music at 64 kb/s is a CELT configuration being coded by the speech codec.

The encoder now splits the same way libopus does. Both read the *same* PCM file — the synthetic chord the test suite generates, written out once so neither side can be fed anything the other was not — and their framing is compared on every packet, not just the first. **1,615 packets across six configurations, all identical**, on length, TOC, packing code and frame count alike. At 48 kHz, `OPUS_APPLICATION_AUDIO`, VBR:

| duration | channels | bitrate | packets compared | libopus's first packet | this crate |
| --- | --- | --- | --- | --- | --- |
| 40 ms | mono | 64 kb/s | 500 | 319 bytes, TOC `f9`, code 1, 2 frames | identical |
| 60 ms | mono | 64 kb/s | 333 | 479 bytes, TOC `fb`, code 3, 3 frames | identical |
| 80 ms | mono | 64 kb/s | 250 | 638 bytes, TOC `fb`, code 3, 4 frames | identical |
| 100 ms | mono | 64 kb/s | 200 | 797 bytes, TOC `fb`, code 3, 5 frames | identical |
| 120 ms | mono | 64 kb/s | 166 | 956 bytes, TOC `fb`, code 3, 6 frames | identical |
| 120 ms | stereo | 96 kb/s | 166 | 1436 bytes, TOC `ff`, code 3, 6 frames | identical |

Every packet decoded back to its full duration, and the decoder's range state matched the encoder's on every one.

One thing this did **not** match:

- At 16 kHz through `OPUS_APPLICATION_VOIP`, libopus picked CELT on this input where we picked SILK. That was present at 20 ms too, where no split is involved, so it was a mode-decision difference rather than a framing one. Since fixed — see [The 16 kHz mode decision](#the-16-khz-mode-decision).
- libopus has a "space too low to do something useful" branch (`opus_encoder.c:1340`) that gives up on coding entirely below 2400 bps at any duration longer than 20 ms, and emits framing with no payload for the decoder to conceal. This crate has no equivalent, and codes what it can instead — 12 bytes of real audio where libopus emits a 2-byte placeholder at 100 ms and 500 bps. The one case where giving up is forced rather than chosen is a packet whose framing alone costs more than its whole CBR budget, which cannot seat its frames at any quality; there this crate emits the same framing libopus does, at the same size (six bytes at 100 ms, eight at 120 ms).

`reference/multiframe/` holds the comparison and the procedure for repeating it; `tests/multiframe_packets.rs` is the part that runs in CI.

### Codec, direction B

All nine `opusenc`-produced files (mono/stereo, 16 and 48 kHz, 24/64/128 kb/s) decode through `OggOpusReader` + `OpusDecoder` to within `2.4e-7` of `opusdec` — 134–138 dB SNR — and to the exact expected sample count. Real `opusenc` files carry end-trimming (a final granule *below* the decodable sample count), so this also exercises that path.

**Our decoder reads real libopus bitstreams correctly.** That is a narrower claim than it looks: `opusenc` never changes coding mode in these files, so direction B exercises nothing at a mode boundary. The direction-A divergences turned out to be a decoder gap after all — see [The mode-switch seam](#the-mode-switch-seam) — and direction B was blind to it.

## Rendering to a different channel count

A stream may be stereo while the caller wants one channel, or mono while the caller wants two, and either can change from packet to packet. The RFC 6716 / RFC 8251 vectors pass **12 of 12 at stereo and 12 of 12 at mono**, with zero range-coder mismatches over 20,075 packets on each leg. `reference/vectors/run.sh` reproduces both.

The mono leg used to fail 6 of those 12, and the shape of the failure is the useful part. Range mismatches were zero there too, so the entropy decode was already right and every divergence was downstream of it, in rendering the stream to a channel count that was not its own.

The cause was that this crate rendered the channel count **after** synthesis, decoding a stereo packet through a *second, complete decoder* and averaging its two output channels. libopus merges **inside each layer, upstream of synthesis**, and keeps one decoder:

- **CELT** (`celt_synthesis` in `celt/celt_decoder.c`) denormalises both channels, sums them, and runs a single inverse MDCT over the sum, into one overlap-add history.
- **SILK** (`silk/dec_API.c`) never reconstructs L/R at all when the API channel count is 1. It emits the mid, which *is* `(L+R)/2` by construction — `silk_stereo_LR_to_MS` computes it as exactly that — and runs one resampler over it. The side channel is still decoded, because its bits have to be consumed, and still extrapolated through a concealed packet.

Measured separately, those two are the smaller half of the story. On a stream that stays stereo throughout, the old and new mono decodes agree to about 106 dB, and on a pure-CELT one to 139 dB — the frequency-domain sum and the time-domain average are the same arithmetic up to float rounding, because everything between the inverse MDCT and the output is linear.

**What did the damage was having two decoders at all.** Each packet went to whichever decoder matched its channel count, so at every mono/stereo switch both were resuming from histories that had missed everything in between. The divergence across the twelve vectors tracks the switch count and nothing else:

| vectors | stereo packets | channel switches | old vs. new |
| --- | --- | --- | --- |
| 12 | 0 % | 0 | identical |
| 01, 11 | 100 % | 0 | 106 dB |
| 02–07 | ~49 % | 1 | 30–63 dB |
| 08, 09, 10 | 52–71 % | 32–64 | 13–16 dB |

The three that switch on nearly every packet are the three that diverged most, and the one that never switches was bit-identical. What looks at first like a correlation with stereo content is really this: a vector is only interesting here if its channel count *changes*.

So the fix is a single decoder, and the in-layer merge is what makes one possible: with the channels combined ahead of synthesis there is one overlap-add, one prefilter, one preemphasis state and one resampler to carry, whatever the packet's channel count is. The auxiliary decoder is gone, along with the state-seeding that tried and failed to keep it continuous; `stream_channels` on the CELT side and `n_channels_internal` on the SILK side follow the stream, and neither is bounded by the output channel count.

Nothing had covered this before. Every other test in the repository decodes at the stream's own channel count, and the vector suite's mono leg had been run with the wrong pass rule — each vector ships two reference decodes and a decode passes if `opus_compare` accepts either — so it had never produced a trustworthy result in either direction.

What covers it now splits the same way the rest of the decoder's conformance does. `tests/decoder_conformance.rs` pins three SILK configurations bit-identically against libopus's own downmix, clean and through a concealed packet, and that runs in CI because SILK is fixed point on both sides. CELT and hybrid decode through float paths that agree with the C decoder to 139–157 dB rather than to the bit, so their evidence is the vector suite, which needs the ~75 MB of vectors fetched: `reference/vectors/run.sh`.

## Defects this found

All were fixed. The largest is treated on its own below; of the rest, three were substantive codec faults:

- **CELT could not code below 48 kHz at all.** It implements only the 48 kHz mode, and a lower API rate was fed straight into it: at 24 kHz the default settings produced a bitstream libopus decoded to a peak of 636,863 for a source peaking at 0.4, in 26 of 32 configurations. libopus handles this by keeping the 48 kHz mode and zero-stuffing the input up to it, so that is what this now does — zero-stuff, limit the coded bands to the input's Nyquist rate, and scale the spectrum back up by the upsampling factor. Both halves matter: without the band limit the mirror image the zero-stuffing creates gets coded and folds back as noise, and without the scaling the output comes back at exactly `1/upsample` of its amplitude. A side effect is that every frame duration from 2.5 to 40 ms now works at every sample rate; the usable set used to depend on the rate.
- **SILK ignored the coded bandwidth when choosing its internal rate**, running at the API rate capped to 16 kHz instead. The TOC then advertised a bandwidth the encoder had not coded, and the decoder ran SILK at a different rate than the encoder — peaks past 113,000. SILK's internal rate now follows the bandwidth as libopus does, which needed the four downsampling filters the port was missing (3:4, 1:2, 1:4 and 1:6, ported from `silk/resampler_rom.c`).
- **40 ms stereo SILK packets were not conformant.** A 40 ms packet holds two SILK frames, and each carries its own stereo prediction weights and mid-only flag. Only the first frame's pair was written, so a conforming decoder parsed everything after it differently. This was invisible at 20 ms, where a packet holds exactly one frame — and invisible in a self-consistent round trip, because this decoder made the same assumption the encoder did. It took a third-party decoder to see it. 40 ms stereo is now bit-identical to libopus.

Three were API faults: `OpusDecoder::decode` treated `frame_size` as an exact output length rather than as the available buffer space; the decoder clamped its float output on the CELT path but not on the SILK path, where libopus clamps neither; and there was no public way to read a packet's duration, which `OggOpusWriter::write_packet` requires — now [`packet::samples_48k`](../src/packet.rs).

Two were container faults:

- **The muxer claimed `pre_skip` more samples than it had written.** The granule counter started at `pre_skip` instead of zero. Because the pre-skip samples are the *first* samples of decoder output, they are already counted by the first packets; seeding the counter with them double-counts. Every file's final granule sat exactly 312 samples past the end of its own audio, so a player computing `final granule - pre_skip` would ask for 6.5 ms that does not exist. Comparing against `opusenc`, whose final granule is *below* the decodable count (legal end-trimming), made the sign of the error obvious. Regression test: `granule_never_claims_more_audio_than_the_packets_carry`.
- **`force_bandwidth` was not capped at the input's Nyquist rate.** It is a user override applied after the automatic selection, so it bypassed every clamp. Asking for superwideband at 8 kHz failed with a misleading complaint about frame sizes (16 configurations), and asking for mediumband at 8 kHz produced a packet that decoded to full-scale noise. libopus applies this clamp last, for the same reason. Fixing it cleared 14 blown-up configurations and all 16 spurious errors. Regression test: `forced_bandwidth_is_capped_by_the_sampling_rate`.

Three things made these findable, and are worth keeping.

A self-consistent round trip proves nothing about conformance: the 40 ms stereo fault round-tripped perfectly through this crate's own decoder, because encoder and decoder shared the mistake.

Peak amplitude is a cheap and blunt oracle — libopus's float API does not clamp, so a correct encode of a source peaking at 0.4 decodes to roughly 0.4, and anything in the thousands is a broken bitstream rather than a tuning difference.

A hand-written SIMD kernel with no differential test is an untested code path, however well covered the code around it is. Two of the kernels described below were wrong, and every end-to-end test passed the whole time.

All three now have tests behind them.

## The hybrid rate split

A hybrid packet carries a SILK low band and a CELT high band in one range-coded stream, and `opus_encoder.c` decides how the rate divides between them before either runs. This port got that division wrong in six places at once. None of it showed up as a conformance failure, because a packet whose bits are split badly is still a valid packet: libopus decodes these at 152 dB either way, the 440-configuration sweep does not move, and the RFC vectors pass 12 of 12 on both legs. It showed up only when [`reference/speed/`](../reference/speed/) started reporting delivered bitrate beside throughput and hybrid came out 12–15% *over* the requested rate where libopus came out 5–14% under.

What was wrong, in descending order of what it cost:

1. **The SILK share was computed at the packet's rate, not per channel.** libopus divides the total by the channel count, reads `compute_silk_rate_for_hybrid`'s table at that single-channel rate, scales the answer back up and trims 1000 for stereo. Reading the table at the full stereo rate lands a row too high, so SILK was handed far more than its share of every stereo hybrid packet.
2. **CELT was given the whole packet's rate** rather than `bitrate_bps - silk_mode.bitRate`. Since the CELT target then adds the bits SILK already coded back in (`target += tell`), the low band was counted twice.
3. **Constrained VBR was left on for the high band.** libopus sets `OPUS_SET_VBR_CONSTRAINT(0)` beside the hybrid bitrate ctl. The constrained path caps the frame against a reservoir sized for the whole packet, which in hybrid is mostly SILK's bits, so it capped the little rate the high band had been given.
4. **`SILKInfo` was never plumbed to CELT.** libopus hands it over before every hybrid frame (`CELT_SET_SILK_INFO`), carrying SILK's signal type and quantization offset. Three behaviours key off it: the tonal-versus-noisy target nudge, `allow_weak_transients`, and the low-bitrate temporal-resolution floor. Its absence also cost `enable_tf_analysis` its `!hybrid` term, so this crate ran a TF analysis on hybrid frames that the reference does not run at all.
5. **The rate table had no FEC columns.** The reference carries a wider SILK share when FEC is on, since the redundant copy costs real bits.
6. **The base rate came from the configured bitrate**, not the frame's own budget — `bits_target`, capped by the caller's buffer and less the TOC byte. At 20 kb/s and 20 ms that is 19600 rather than 20000, which is a whole table row's worth of interpolation.

### Where the bits were going

The boundary between the two layers is not observable from outside a decoder, so `reference/speed/split` reads it out of this crate's decoder at the point SILK finishes. That decoder matches libopus bit for bit on hybrid, so pointing it at the reference's packets measures the reference's split too. At 20 kb/s fullband mono, in bits per frame:

| | SILK low band | CELT high band | delivered |
| --- | --- | --- | --- |
| before | 227 | **54** | 22.3 kb/s |
| after | 227 | **97** | 16.2 kb/s |
| libopus | 215 | 129 | 17.2 kb/s |

SILK was never far off. The high band was starved throughout, and the overshoot in the "before" row is the double-counted low band rather than a generous high one.

### What it was worth

Encoded here, decoded by libopus, against the source:

| | before | after | libopus |
| --- | --- | --- | --- |
| hybrid SWB 12 kb/s mono | 13.8 kb/s, 5.52 dB | 10.3 kb/s, 5.46 dB | 11.4 kb/s, 5.38 dB |
| hybrid FB 20 kb/s mono | 22.3 kb/s, 5.56 dB | 16.2 kb/s, 5.55 dB | 17.2 kb/s, 5.49 dB |
| hybrid FB 24 kb/s stereo | 26.8 kb/s, **2.61 dB** | 21.1 kb/s, **4.27 dB** | 22.6 kb/s, 4.28 dB |

Mono holds its quality on a quarter fewer bits. Stereo gains 1.66 dB while spending 21% less, landing on libopus's 4.28: the per-channel error was not merely mis-spending the budget, it was degrading stereo hybrid.

The speed table corroborates it from the other side. `hybrid FB 24 kb/s stereo` used to encode at 1.82x libopus where every other hybrid row sat near 1.12x, and that was recorded at the time as the one row whose number to distrust was ours. It was: libopus pays about a 1.9x penalty going from mono to stereo in hybrid, this crate was paying 1.16x, and the difference was work it was not doing. The row now reads 1.14x.

### What is left

Hybrid now lands 6–10% under libopus rather than 12–15% over. The residual splits in two: this crate's SILK spends about 6% more than the reference's, which the SILK-only rows show too (+6 to +11%) and which is therefore not a hybrid question; and the high band still codes 97 bits per frame against 129, which is.

Those 32 bits do not buy high-band quality. At these rates the band is below the rate at which CELT codes a waveform at all — it is filled with shaped noise — and measured per band in [`reference/highband/`](../reference/highband/), this crate's high band comes back within half a dB of the source's level and tracks its envelope at least as tightly as libopus's. libopus spends the difference quieting the band in proportion to how little it could afford to code, from −4 dB at 12 kb/s to −0.4 dB at 64. Which is preferable is a listening question. The bit gap itself is a constant 30 to 32 bits per frame across a five-fold rate range, not a widening allocation difference.

Nothing in the frozen tables had been hybrid — the 48 kHz music entries are CELT and the 8 and 16 kHz speech ones SILK — which is why this drifted unreported. `tests/bitstream_stability.rs` now carries a mono and a stereo hybrid configuration, the stereo one specifically for the per-channel allocation.

## The mode-switch seam

The first sweep left 21 configurations agreeing with libopus to only ~25 dB. Each was a stream whose encoder changed coding mode mid-file, and in each the divergence sat inside one 20 ms frame — the one at the boundary — where the worst sample was off by 1.22 on a source peaking at 0.4. That is not a tuning difference; that is a hole in the audio.

The cause was a missing mechanism rather than a wrong number. When the coding mode changes, the layer taking over has no overlap-add history and its first samples climb out of silence. libopus covers the seam by *concealing* 5 ms in the mode being left behind and cross-fading that over the head of the new frame (`opus_decoder.c`, `pcm_transition`). Nothing here did that, so the first 2.5 ms of every transition frame decoded to roughly nothing.

Building it exposed three more faults underneath, each of which had been invisible because nothing exercised concealment:

- **`pitch_downsample` did not decimate on aarch64.** Its vectorised path issued one `vld1q` per filter tap and stored four outputs per iteration, which walks the input one sample per output rather than two. Only the first lane of each group of four landed on the right input sample. The pitch estimate built on that buffer was wrong, and the encoder's prefilter analysis reads the same buffer.
- **`celt_fir` added its input four times on aarch64.** The NEON kernel seeded its four-lane accumulator with `x[i]` broadcast across all lanes, so the horizontal reduction counted `x[i]` four times over. Its one caller is the pitch-based concealment, where it inflated the LPC residual by roughly 3x until libopus's own explosion guard (`!(S1 > 0.2*S2)`) zeroed the frame — turning concealment into silence on every ARM build.
- **The decoder's history buffer was 3072 samples where libopus uses 2048.** That buffer doubles as the window the concealment searches for a pitch period, so the larger size silently handed the search 50% more signal and it settled on a different lag.

Both SIMD faults are now pinned by tests that compare each kernel against the scalar definition it stands in for, and both were confirmed to fail those tests when reintroduced. They are a reminder that a hand-written SIMD kernel with no differential test is an untested code path, however well the codec around it is covered — the codec's own end-to-end tests passed throughout. That reminder was taken seriously enough to sweep the rest; see [The SIMD audit](#the-simd-audit).

With all four fixed, the transition frame's worst sample went from 1.22 to 9.4e-5 and the reproducer stream from 26 dB to 107 dB. What was left after that was the concealed content itself, confined to the 5 ms window — a separate subsystem, since taken apart against the reference in [Packet-loss concealment](#packet-loss-concealment).

One 48 kHz stereo stream at 16 kb/s sat at 56 dB even after that, until the stereo predictors were carried through concealment: libopus holds the previous frame's mid/side prediction weights across a concealed frame, where this decoder had been interpolating them to zero, which narrowed the image over exactly the window the cross-fade reads. With that fixed the stream agrees to better than 100 dB.

On the current sweep the residue is 12 configurations of 280 at 20 ms and 4 of 160 at 60 ms, worst 80 dB, all of them mode-switching streams. Its shape has changed since the paragraphs above were written — the encoder now switches where it used to be pinned, so hybrid-to-CELT seams appear where SILK-to-CELT ones did — but it is the same mechanism at the same order, ~1e-3 at worst. What is new is that the confinement is now measured rather than asserted: across all 440 streams, in both sweeps, not one sample differs by more than 1e-5 anywhere except inside a single sub-5 ms window at a switch.

The three moved entries in `tests/bitstream_stability.rs` are a consequence of the `pitch_downsample` fix: the CELT prefilter signals a different pitch, so the mono CELT bitstreams changed. Only mono moved — the stereo path never took the vectorised branch — and on x86-64, where the scalar path already ran, nothing changed at all.

## The SIMD audit

Two of the first five kernels examined were wrong, so the remaining sixty-odd were swept the same way: a test per kernel comparing the *dispatcher* against the scalar definition it implements, which means an aarch64 build checks its NEON kernels and an x86-64 build its SSE and AVX ones. `cargo test --target x86_64-apple-darwin` runs the suite under Rosetta, which covers SSE and the scalar fallbacks; the AVX2 paths still need real x86 hardware.

The sweep found three more faults and two pieces of dead weight:

- **`pitch_downsample` never whitened off aarch64.** libopus decimates and then applies an order-4 LPC whitening before anything searches the buffer for a pitch period. That second stage lived inside the NEON function, so every other target searched an unwhitened signal. Found on the first x86-64 run.
- **The NEON PVQ search picked worse codewords.** Its short-codeword branch maximised `Rxy/Ryy` where the reference maximises `Rxy^2/Ryy`; its longer branch scored with `vrsqrteq_f32`, an eight-bit estimate, and broke ties by float equality against a running maximum. Measured against a transcription of `op_pvq_search_c` it differed on 470 of 1880 cases and scored worse on 450, at up to 29.7% of the objective. Nothing end-to-end could see this: the decoder reconstructs whatever codeword the encoder chose, so a bad search costs quality silently. Removed in favour of the scalar search, which is exact — libopus does not vectorise this on ARM either. Round-trip SNR rose by up to 0.49 dB at unchanged bitrate for about 1.5% more encode time.
- **`silk_sum_sqr_shift` truncated per sample rather than per pair on aarch64**, drifting the energy low. SILK is fixed point, so exactness is the contract rather than a nicety.
- **`haar1`'s AVX and NEON paths** had a deinterleave bug, had been disabled in place, and sat unreachable behind a scalar dispatcher. **All three `stereo_merge` variants** had no callers at all. Both are gone; a future vectorisation belongs in the tree with a test beside it, not as a second implementation the dispatcher declines to call.

One more thing surfaced along the way: `pvq_search_scalar` bounded its fixed 32-entry working buffers with a `debug_assert!`, which release builds compile out — a safe function that corrupts memory if its contract is ever broken. The bound is now enforced.

Three paths remain deliberate approximations rather than faults: the n = 2 and n = 4 closed-form PVQ searches and the fast-select search used from n = 32 up. They behave identically on every architecture and give up at most 1.7%, 3.3% and 2.0% of the search objective respectively. Those figures are now pinned by a test so the trade cannot quietly widen.

One kernel test did not follow the pattern, and it took a year-later x86-64 run to notice. `kiss_fft`'s `m == 1` radix-2 and radix-4 butterflies are the crate's only aarch64-exclusive kernels, and their scalar counterparts were written inline inside `#[cfg(not(target_arch = "aarch64"))]` blocks rather than as named functions. The test could therefore only call the NEON form, behind a `cfg`, so on x86-64 it compared untransformed input against a transformed reference and could not pass. Both scalar kernels are now named functions that the dispatcher and the test share, and the test checks the scalar form on every target and the NEON form wherever it exists. Sabotaging the scalar kernel fails the test on both architectures; the change itself moved no bytes on x86-64, where that code is what actually runs.

The remaining limitations are recorded under [Known limitations](../README.md#known-limitations).

## The 16 kHz mode decision

libopus chooses between SILK and CELT by comparing the equivalent rate against a threshold, and nothing else (`opus_encoder.c`). This port carried an extra `&& self.sampling_rate >= 24000` from the crate it was forked from, where CELT below 48 kHz was broken — 24 kHz decoded to full-scale noise. That was fixed here when CELT learned to code lower rates the way libopus does, but the guard outlived the bug and pinned 8, 12 and 16 kHz to SILK whatever the content or bitrate.

The cost was not only mode choice. SILK saturates at wideband, so the requested bitrate was never spent. At 16 kHz mono through `OPUS_APPLICATION_VOIP`:

| asked for | delivered before | delivered after | round-trip SNR before | after |
| --- | --- | --- | --- | --- |
| 48 kb/s | 35.4 kb/s | 48.3 kb/s | 14.3 dB | 13.9 dB |
| 96 kb/s | 61.7 kb/s | 96.4 kb/s | 14.4 dB | **50.8 dB** |

With the guard removed, the mode decision matches libopus across a 30-configuration sweep (8, 12 and 16 kHz; both applications; 16 to 64 kb/s): 27 agree exactly, and the 3 that differ are 16 kHz `AUDIO` at 24 and 32 kb/s, where libopus spends about 21 of 1000 packets in SILK before switching and this crate switches immediately. That is a marginal threshold crossing during the tonality analysis's warm-up, not a structural difference.

Decoded quality against libopus on the same audio, `AUDIO` application, after the change:

| rate | 16 kb/s | 24 kb/s | 48 kb/s |
| --- | --- | --- | --- |
| 8 kHz | 21.9 / **22.2** | 23.8 / **28.6** | 38.0 / **44.6** |
| 12 kHz | 21.1 / **22.2** | 20.6 / **29.5** | 29.5 / **44.4** |
| 16 kHz | **13.9** / 12.6 | 24.2 / **28.6** | 35.7 / **41.1** |

(libopus dB / this crate dB; the better of each pair in bold. The one case where libopus leads is 1.3 dB at 16 kHz and 16 kb/s, where both make the same mode choice.)

One consequence reaches the TOC. CELT has no mediumband configuration, so libopus applies the Nyquist clamps and then widens a surviving mediumband request to wideband. At 12 kHz the clamp caps every request at mediumband, so under CELT any request from mediumband up emerges as wideband. Measured at 12 kHz, 48 kb/s, music, forcing each bandwidth in turn, this crate and libopus produce identical TOCs: config 19 for narrowband, config 23 for mediumband, wideband, superwideband and fullband alike. At 8 kHz both produce config 19 throughout. `forced_bandwidth_is_capped_by_the_sampling_rate` pins that rule.

No frozen bitstream moved: those configurations are speech at 8 and 16 kHz, which stay in SILK, or 24 and 48 kHz, which were never behind the guard.

## The analysis warm-up guard

This port discounted the tonality classifier's verdict for its first ten frames, falling back to the application default, on the stated theory that libopus's analysis lookahead leaves its classifier converged by frame 0. It does not: libopus spends about twenty frames climbing from "voice" to "music" on musical input, which is why its first ~20 packets of music at a marginal bitrate come out hybrid or SILK before it settles on CELT. The early hybrid run the guard was added to remove was the reference's own behaviour.

The guard also had a cost. Across 32 configurations — 8, 12, 16 and 48 kHz, both applications, 16 to 48 kb/s — trusting the verdict from the first frame gives 20 exact matches with libopus's mode sequence and 12 within a single packet, none further out. The guard put the same sweep as much as twenty-one packets out.

Removing it was deliberately sequenced after [the CELT input delay](#the-celt-input-delay). The guard kept early streams single-mode, which hid the 4 ms timeline jump at a mode switch; with the delay fixed, both the SILK→CELT seam at 16 kHz and the hybrid→CELT seam at 48 kHz hold a constant lag through either decoder. No frozen bitstream moved, since the entries that could be warm-up sensitive are all at bitrates high enough to pick CELT from the first frame regardless.

## The CELT input delay

CELT has a shorter algorithmic delay than SILK, so libopus hands it input that lags the caller's by `Fs/250` — 4 ms. It builds `pcm_buf` as that much history followed by the new frame, gives SILK the new frame (`opus_encoder.c:2211`) and CELT the buffer from the start (`:2493`). The two layers then line up at the decoder, and the constant total delay of `Fs/400 + Fs/250` is what `OpusHead::RECOMMENDED_PRE_SKIP` counts: 312 samples at 48 kHz.

This port had no delay buffer at all. CELT was fed the newest samples, so:

- every CELT-only stream ran 4 ms ahead of libopus;
- the output timeline **jumped 4 ms at every SILK↔CELT switch**, because the SILK path was correctly aligned and the CELT path was not;
- the Ogg header declared a 312-sample pre-skip against an encoder whose CELT delay was only `Fs/400` — 120 samples at 48 kHz — so a player trimming what the header asked for dropped 4 ms of real audio.

Measured by decoding each encoder's own stream with libopus and finding the lag that best aligns it to the source. Before:

| rate | opus-pure | libopus | difference | `Fs/250` |
| --- | --- | --- | --- | --- |
| 16 kHz | 40 | 104 | 64 | 64 |
| 24 kHz | 60 | 156 | 96 | 96 |
| 48 kHz | 120 | 312 | 192 | 192 |

After the fix all three match libopus exactly — 104, 156 and 312.

The mode-switch jump was the sharper symptom. On a stream that runs speech for two seconds and then music, with the SILK→CELT switch at packet 31, measuring the SILK and CELT stretches separately:

| encoder → decoder | SILK stretch | CELT stretch |
| --- | --- | --- |
| libopus → libopus | lag 104 | lag 104 |
| libopus → opus-pure | lag 104 | lag 104 |
| opus-pure → libopus | lag 104 | lag 40 |
| opus-pure → opus-pure | lag 104 | lag 40 |

libopus's decoder saw the same jump in our bitstream that ours did, and our decoder handled libopus's stream correctly, which is what identified the encoder rather than the decoder. All four rows now read 104 on both sides.

Nothing in the suite could see any of this. Every fidelity assertion measures through a lag search, which absorbs a constant offset — the same blind spot that hid the SILK resampler delay recorded in `tests/decoder_conformance.rs`. Two tests had to be corrected to tolerate the now-longer delay rather than the other way round: `all_frame_durations_work_at_all_sample_rates` searched only one frame of lag, which at 2.5 ms is shorter than the codec's own delay, and `every_frame_of_a_split_packet_carries_its_own_audio` assumed the delay fit inside a 2.5 ms slice guard and now measures it instead.

The eight CELT and hybrid entries in `tests/bitstream_stability.rs` moved; the three SILK entries did not, which is the shape of the fix showing in the table.

`tests/encoder_delay.rs` now asserts the delay as a number rather than searching for it, so a third defect of this kind fails in CI instead of hiding.

### The doubled SILK resampler delay

Fixing the CELT input delay left a smaller offset behind, and it turned out not to be about hybrid at all.

The symptom was that this crate's hybrid output sat 28 samples further back than libopus's at 48 kHz, 14 at 24 kHz — the same 0.58 ms at both. Splitting the decoded output into its two halves at the 8 kHz crossover said which layer was late: the CELT high band measured 311.99 against the expected 312, and the SILK low band measured 330, carrying 99.8% of the energy and dragging the full-band figure with it. So the defect was in SILK, not in the hybrid combination, and measuring SILK-only streams confirmed it — at 48 kHz they were late by the same amount.

Against libopus, SILK-only delay came out:

| API rate | SILK internal | this crate | libopus | difference |
| --- | --- | --- | --- | --- |
| 8 kHz | 8 kHz | 50.18 | 50.17 | 0 |
| 12 kHz | 12 kHz | 76.25 | 76.23 | 0 |
| 16 kHz | 16 kHz | 102.31 | 102.31 | 0 |
| 24 kHz | 16 kHz | 168.13 | 153.06 | **15** |
| 48 kHz | 16 kHz | 337.81 | 307.72 | **30** |

Zero wherever the API rate already equals SILK's internal rate, and otherwise exactly 10 samples of SILK-internal rate. That shape names the cause. `silk_resampler_init` picks a kernel and one of the choices is `USE_silk_resampler_copy`, which is not a copy: it runs the delay buffer, because `delay_matrix_enc` equalises the codec's total delay across every rate pair and the equal-rate pairs carry non-zero entries (6, 7 and 10 at 8, 12 and 16 kHz). This port had split that in two. `encoder.rs` resampled with `SilkDownFirResampler`, which applied `delay_matrix_enc[API][internal]` correctly, and then `silk_encode` applied a second delay buffer of its own using `delay_matrix_enc[internal][internal]`. When the rates matched, the second one was the only one and was right. When they did not, the stream got both.

An impulse through libopus's own resampler shows the equalisation working: 48000→16000, 24000→16000 and 16000→16000 all land the peak at output sample 330, whatever the input rate.

The fix gives the delay one owner. `SilkEncoderResampler` chooses between a ratio kernel and a pass-through, both of which run the delay buffer, and `silk_encode` no longer has one. Every rate now lands within 0.07 samples of libopus, hybrid included, and the mode-switch test's allowance for hybrid dropped from 28 samples to the 2 every other mode already met.

**Why it survived.** All 14 byte-exact configurations in `tests/reference_vectors.rs` ran at 8, 12 or 16 kHz, and so do the three SILK entries in `tests/bitstream_stability.rs`. Those are exactly the rates where the API rate equals SILK's internal rate — so the resampler's ratio path, the one every 24 and 48 kHz SILK and hybrid stream goes through, had no reference test at all. `silk_lands_where_celt_does_at_every_rate` now pins the property across all five rates, and reintroducing the second delay buffer fails it along with `the_delay_does_not_move_when_the_mode_does`.

**The gap itself is closed.** `tests/reference_vectors.rs` carries eight more configurations, generated the same way as the original fourteen and byte-exact against the same fixed-point libopus 1.6.1. Forcing the bandwidth pins SILK's internal rate, so the six at complexity 5 cover every ratio the encoder can ask for — 24000 and 48000 down to each of 8000, 12000 and 16000 — with wideband repeated at complexity 10 at both rates:

| API rate | SILK internal | forced bandwidth | test |
|---|---|---|---|
| 24 kHz | 8 kHz | narrowband | `test_api24k_nb_c5` |
| 24 kHz | 12 kHz | mediumband | `test_api24k_mb_c5` |
| 24 kHz | 16 kHz | wideband | `test_api24k_wb_c5`, `test_api24k_wb_c10` |
| 48 kHz | 8 kHz | narrowband | `test_api48k_nb_c5` |
| 48 kHz | 12 kHz | mediumband | `test_api48k_mb_c5` |
| 48 kHz | 16 kHz | wideband | `test_api48k_wb_c5`, `test_api48k_wb_c10` |

All eight matched on the first run, so this crate's resampler is bit-exact with the reference's and so is everything downstream of it: the resampled signal is what the pitch analysis, LSF search and quantiser all see. Perturbing the ratio path's `input_delay` by a single sample fails exactly those eight and none of the original fourteen, which is the coverage gap stated as a measurement rather than as a claim.

Generating them needs a **fixed-point** libopus (`-DOPUS_FIXED_POINT=ON`); a float build takes SILK's float path and will not match. `reference/vectors/README.md` has the recipe and `reference/vectors/cvec.c` is the generator.

## Packet-loss concealment

Concealment had never been held against the reference directly. Every test measured it through the audio around it — the mode-switch seam, the FEC comparisons, a per-frame energy check — and all of those pass whether the concealed samples are right or merely plausible. `reference/plc/` now decodes the same packets twice, once through this crate and once through libopus 1.6.1, dropping the same packets in both, and compares the two dumps sample for sample.

The first run said what that kind of test is for. **Concealed frames matched bit for bit; the frames after them did not.** At 8, 12 and 16 kHz the concealed frame was identical and the first good frame after it was 22-28 dB from libopus, decaying back to agreement over about thirteen frames. A defect that only shows up in the recovery is exactly the shape no existing test could see.

### The four SILK defects

- **`BWE_AFTER_LOSS_Q16` was 64738 where libopus has 63570.** The first good frame after a loss has its LPC coefficients bandwidth-expanded to keep a predictor built partly from invented signal from ringing; the constant sets how much. 0.988 instead of 0.97 is a small difference per coefficient and a large one by the sixteenth: on the reproducer the first two coefficients came out 5836, -2268 against the reference's 5731, -2187.
- **The gain dequantiser was not re-anchored after a loss.** `silk_Decode` sets `LastGainIndex = 10` on a concealed packet (dec_API.c), because an independently coded gain is clamped to `LastGainIndex - 16` and a stale index from before the loss holds the recovered frame's level up when the signal was on its way down — the "energy bounce back" the comment there names. Nothing else resets that index.
- **Concealment ran before the first packet had been decoded.** libopus returns silence and touches no decoder state when it has no previous mode to conceal in (`if (mode == 0)`, opus_decoder.c). Concealing in a guessed mode produces the same silence but advances SILK a frame: `lossCnt` bumped, output history rotated, PLC seed stepped. The first real packet then decoded its LPC coefficients through an after-loss bandwidth expansion that libopus had no reason to apply.
- **The voiced-to-unvoiced fade was coded with zero gain.** After concealing a voiced frame, libopus codes the first half of the next frame as voiced at the concealment's own lag with a fixed 0.25 long-term gain (decode_core.c, "Avoid abrupt transition from voiced PLC to unvoiced normal decoding"), so the periodicity fades out instead of stopping mid-pitch-period. This decoder took the lag and the signal type but left the LTP gains at the decoded zeros, which is the transition that code exists to avoid.

With those four fixed, **SILK concealment is bit-identical to libopus** at 8, 12 and 16 kHz, mono and stereo, at 10, 20, 40 and 60 ms, for single losses, bursts of up to ten, scattered losses, and a loss arriving before the first packet. Twenty configurations were checked by hand and nine are frozen in `tests/decoder_conformance.rs`, so CI holds them. SILK is fixed point on both sides: it agrees exactly or something is wrong.

### CELT

The CELT half was a port of an older libopus. Against 1.6.1 it fell back to noise-based concealment after a fixed number of lost *frames* rather than after 100 ms of loss, decayed band energy towards silence rather than towards the measured noise floor, folded its MDCT overlap immediately with its own postfilter parameters instead of deferring the fold to whatever decoded next, ran no postfilter over a noise-concealed frame, and left the postfilter's history at the last *decoded* frame — so the frame after a loss was comb-filtered against audio a frame out of place.

One more was structural. libopus allocates the band-energy arrays two channels wide whatever the decoder is, and a mono decoder uses the second half as a snapshot: `OPUS_COPY(&oldBandE[nbEBands], oldBandE, nbEBands)` at the end of every decoded frame, `oldBandE[i] = MAX(oldBandE[i], oldBandE[nbEBands+i])` at the start of the next. Concealment writes only the first half, so the merge restores the last decoded frame's coarse energy over whatever the concealment decayed it to. This decoder sized those arrays by channel count, so a mono decoder had no second half and no snapshot, and the first frame back from a burst predicted its energy from concealment's decayed state.

| | before | after |
|---|---|---|
| 48 kHz mono, one loss | 28 dB | 112 dB |
| 48 kHz stereo, one loss | 30 dB | 85 dB |
| 48 kHz mono, burst of 8 | 18 dB | 83 dB |
| 24 kHz hybrid mono | 63 dB | 157 dB |
| 48 kHz hybrid mono | 55 dB | 153 dB |

### Why CELT stops short of exact

A concealed CELT frame agrees with libopus to 83-112 dB where an ordinary one agrees to 139, and that gap is not a missing step. The pitch-based branch fits a 24th-order LPC to the autocorrelation of 1024 samples; summing 1024 float products in a different order changes that autocorrelation in its last bits, and the Levinson-Durbin recursion at order 24 turns it into something three orders of magnitude larger. Measured on the reproducer: `ac[0]` differs by 4e-7 relative, the resulting first LPC coefficient by 3.6e-4, and the concealed output by about 1e-5 — flat across the frame, which is what a slightly different filter looks like rather than a diverging one.

Closing that would mean matching libopus's `celt_pitch_xcorr` accumulation order bit for bit, which libopus does not do across its own architectures. Everything above the arithmetic — which branch runs, how long before noise takes over, what state each leaves behind, every flag and counter — was compared against a traced reference build frame by frame and agrees exactly.

## Reproducible test signals

Frozen-hash tests only mean something while their input means something, and this suite's input did not carry across architectures.

`speech_like`, `music_like` and `sine` were built with `f32::sin` and `f32::powf`. Both call the platform's libm, and Apple's arm64 and x86_64 implementations differ in the last bits, so every generated signal hashed differently on the two machines. Tolerance-based assertions never noticed. The frozen ones did: `decoder_conformance`'s six SILK configurations are hashes of libopus 1.6.1's decode of our packets, and at 12 kHz stereo the input difference was enough to move SILK's quantiser across a decision boundary. That entry failed on x86-64 while the file passed on aarch64. The other five held by luck rather than by construction, and so did every entry in `bitstream_stability`.

The generators now use a polynomial sine built from correctly-rounded operations alone (`tests/common/sin_turns`), with the argument taken in turns so the range reduction is exact. `tests/test_signals.rs` freezes what they produce and checks the sine against the identities that define it. With identical input on both machines:

- All six `decoder_conformance` configurations are bit-identical to libopus 1.6.1 on aarch64 **and** x86-64. The SILK decoder's bit-exactness is now a demonstrated property rather than an assumption.
- All three SILK entries in `bitstream_stability` are byte-identical across architectures.
- The five CELT entries still differ, as they should: that layer is float and its NEON and scalar kernels sum the same terms in different orders. Byte *lengths* match everywhere. libopus has the same property, which is why its conformance is defined on the fixed-point decoder rather than on bit-equality of the float encoder.

## Reproducing the direction-A numbers

Every figure in the two direction-A tables comes from one script, `reference/interop/run_sweep.sh`, which regenerates all 440 files from a clean tree, validates them with `opusinfo`, decodes each with both stacks and prints the tables. It needs `opus-tools` on `PATH` and python3; `reference/interop/README.md` describes the matrices and the tools alongside it. The matrix:

| matrix | files | configurations |
| --- | --- | --- |
| 20 ms bandwidth | 120 | 5 rates x mono/stereo x 6 forced bandwidths x 2 applications, 48 kb/s, music |
| 20 ms bitrate | 160 | 5 rates x mono/stereo x 16/32/64/96 kb/s x speech/music x 2 applications, bandwidth auto |
| 60 ms | 160 | 5 rates x mono/stereo x 4 bandwidths x 2 applications x 16/48 kb/s, music |

Rates are 8, 12, 16, 24 and 48 kHz. Signals come from `tests/common/mod.rs`, included by path rather than copied, so the sweep encodes exactly what `cargo test` encodes and the numbers do not depend on the host's libm.

Both decodes run at 48 kHz with pre-skip and end-trimming applied identically. The difference threshold for "these samples disagree" is 1e-5, about two orders above the float32 rounding these signals produce.

## Measurement notes

Peak amplitude is a useful health check because libopus's float API does not clamp: a correct encode of a source peaking at 0.4 decodes to roughly 0.4, with mild overshoot (up to 2.3 across these sweeps) from normal codec ringing. Anything far above that is a broken bitstream rather than a tuning difference. The blown-up configurations reach 636,863.

The check that caught the granule bug is worth keeping in mind: sum each packet's duration from its own TOC byte and compare against the final granule. The difference must be zero (exact) or negative (end-trimming). Positive means the file promises audio it does not contain.
