//! Bakes `framework_shaders::SOURCES_KEY`: FNV-1a over the name and text of
//! every `.wgsl` file in `shaders/`, in name order, so it changes when any
//! shader does and with nothing else.

use std::{env, ffi::OsStr, fs, io, path::Path};

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

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=shaders");
    let dir = Path::new("shaders");
    let mut names = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    names.retain(|name| Path::new(name).extension() == Some(OsStr::new("wgsl")));
    names.sort_unstable();
    let key = names.iter().try_fold(FNV_OFFSET, |key, name| {
        let text = fs::read(dir.join(name))?;
        io::Result::Ok(fold(fold(key, name.as_encoded_bytes()), &text))
    })?;
    let out = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR unset"))?;
    fs::write(
        Path::new(&out).join("framework_shaders_key.rs"),
        format!("{key:#018x}"),
    )
}
