use super::*;

fn parse(codec: VideoCodec, data: &[u8]) -> AccessUnitNals {
    let mut nals = AccessUnitNals::default();
    nals.parse(codec, data);
    nals
}

fn prefixed(nals: &AccessUnitNals, data: &[u8]) -> Vec<u8> {
    let mut out = vec![0; nals.length_prefixed_len()];
    nals.write_length_prefixed(data, &mut out);
    out
}

#[test]
fn start_codes_of_three_and_four_bytes_split_the_units() {
    // AUD, then an IDR slice behind a four-byte start code, then a plain slice behind three
    let data = [
        0, 0, 0, 1, 0x09, 0xf0, 0, 0, 0, 1, 0x65, 1, 2, 3, 0, 0, 1, 0x41, 4, 5,
    ];
    let nals = parse(VideoCodec::H264, &data);
    assert!(nals.has_keyframe());
    assert_eq!(
        prefixed(&nals, &data),
        [0, 0, 0, 4, 0x65, 1, 2, 3, 0, 0, 0, 3, 0x41, 4, 5]
    );
}

#[test]
fn zeros_before_a_start_code_do_not_belong_to_the_unit() {
    let data = [0, 0, 1, 0x41, 7, 0, 0, 0, 0, 0, 1, 0x41, 8];
    let nals = parse(VideoCodec::H264, &data);
    assert_eq!(
        prefixed(&nals, &data),
        [0, 0, 0, 2, 0x41, 7, 0, 0, 0, 2, 0x41, 8]
    );
}

#[test]
fn bytes_before_the_first_start_code_belong_to_no_unit() {
    let data = [9, 9, 0, 0, 1, 0x41, 1];
    let nals = parse(VideoCodec::H264, &data);
    assert_eq!(prefixed(&nals, &data), [0, 0, 0, 2, 0x41, 1]);
}

#[test]
fn a_unit_of_parameter_sets_has_no_picture() {
    let data = [0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2];
    let nals = parse(VideoCodec::H264, &data);
    assert!(!nals.has_picture());
    let sets: Vec<&[u8]> = nals
        .parameter_sets(VideoCodec::H264, &data)
        .expect("a full set")
        .collect();
    assert_eq!(sets, [&[0x67, 1][..], &[0x68, 2][..]]);
}

#[test]
fn a_part_of_a_parameter_set_is_no_set() {
    let h264 = [0, 0, 0, 1, 0x67, 1];
    assert!(
        parse(VideoCodec::H264, &h264)
            .parameter_sets(VideoCodec::H264, &h264)
            .is_none()
    );
    // HEVC needs the video parameter set too: SPS and PPS alone are not a set
    let hevc = [0, 0, 0, 1, 0x42, 1, 0, 0, 0, 1, 0x44, 1];
    assert!(
        parse(VideoCodec::Hevc, &hevc)
            .parameter_sets(VideoCodec::Hevc, &hevc)
            .is_none()
    );
}

#[test]
fn hevc_units_are_told_by_their_type_bits() {
    // VPS 32, SPS 33, PPS 34, AUD 35, IDR_W_RADL 19, TRAIL_R 1
    let data = [
        0, 0, 0, 1, 0x40, 1, 0, 0, 0, 1, 0x42, 1, 0, 0, 0, 1, 0x44, 1, 0, 0, 0, 1, 0x46, 1, 0, 0,
        0, 1, 0x26, 1, 9, 0, 0, 1, 0x02, 1, 8,
    ];
    let nals = parse(VideoCodec::Hevc, &data);
    assert!(nals.has_keyframe());
    assert_eq!(
        nals.parameter_sets(VideoCodec::Hevc, &data)
            .map(Iterator::count),
        Some(3)
    );
    assert_eq!(
        prefixed(&nals, &data),
        [0, 0, 0, 3, 0x26, 1, 9, 0, 0, 0, 3, 0x02, 1, 8]
    );
}

#[test]
fn a_plain_slice_is_no_keyframe() {
    let data = [0, 0, 1, 0x41, 1, 0, 0, 1, 0x06, 2];
    let nals = parse(VideoCodec::H264, &data);
    assert!(nals.has_picture());
    assert!(!nals.has_keyframe());
}
