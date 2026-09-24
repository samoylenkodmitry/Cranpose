use super::*;

#[test]
fn random_u32_never_returns_zero() {
    for _ in 0..100 {
        assert!(random_u32(100) >= 1);
    }
}

#[test]
fn decode_bitmap_rejects_invalid_bytes() {
    assert!(decode_bitmap(&[1, 2, 3, 4]).is_err());
}
