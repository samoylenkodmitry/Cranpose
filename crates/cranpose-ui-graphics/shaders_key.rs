//! The key of the framework's WGSL files, shared by the build script that
//! bakes it into the crate and the test that checks it.

use std::{ffi::OsStr, fs, io, path::Path};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Folds `bytes` and their length, so adjacent fields cannot trade bytes.
fn fold(hash: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .chain(&(bytes.len() as u64).to_le_bytes())
        .fold(hash, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        })
}

/// FNV-1a over the name and text of every `.wgsl` file in `dir`, in name
/// order, so it changes when any shader does and with nothing else.
pub fn shaders_key(dir: &Path) -> io::Result<u64> {
    let mut names = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    names.retain(|name| Path::new(name).extension() == Some(OsStr::new("wgsl")));
    names.sort_unstable();
    names.iter().try_fold(FNV_OFFSET, |key, name| {
        let text = fs::read(dir.join(name))?;
        Ok(fold(fold(key, name.as_encoded_bytes()), &text))
    })
}
