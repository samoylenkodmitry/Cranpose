use super::*;

pub fn location_key(file: &str, line: u32, column: u32) -> Key {
    let base = file.as_ptr() as u64;
    base.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ ((line as u64) << 32) ^ (column as u64)
}
