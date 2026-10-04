//! Writes every `.wgsl` file in `shaders/` to `OUT_DIR/shaders` without its
//! comments or the whitespace around each line, keeping every line so a
//! compiler's line numbers still point into the source file. Bakes
//! `framework_shaders::SOURCES_KEY`: FNV-1a over the name and stripped text
//! of each file, in name order, so it changes when a shader's code does and
//! with nothing else.

use std::{env, ffi::OsStr, fs, io, path::Path};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A comment opening with this is a directive the renderer looks for in the
/// shader text, and stays.
const DIRECTIVE: &str = "//@";

/// Folds `bytes` and their length, so adjacent fields cannot trade bytes.
fn fold(hash: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .chain(&(bytes.len() as u64).to_le_bytes())
        .fold(hash, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        })
}

/// `text` without comments or the whitespace around each line. WGSL has no
/// string literals, so `//` always opens a comment.
fn strip(name: &OsStr, text: &str) -> io::Result<String> {
    if text.contains("/*") {
        return Err(io::Error::other(format!(
            "{}: block comments are not stripped; use `//`",
            name.display()
        )));
    }
    let mut stripped = String::with_capacity(text.len());
    for line in text.lines() {
        let code = match line.find("//") {
            Some(at) if !line[at..].starts_with(DIRECTIVE) => &line[..at],
            _ => line,
        };
        stripped.push_str(code.trim());
        stripped.push('\n');
    }
    Ok(stripped)
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
    let out = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR unset"))?;
    let out = Path::new(&out);
    fs::create_dir_all(out.join("shaders"))?;
    let key = names.iter().try_fold(FNV_OFFSET, |key, name| {
        let stripped = strip(name, &fs::read_to_string(dir.join(name))?)?;
        fs::write(out.join("shaders").join(name), &stripped)?;
        io::Result::Ok(fold(
            fold(key, name.as_encoded_bytes()),
            stripped.as_bytes(),
        ))
    })?;
    fs::write(out.join("framework_shaders_key.rs"), format!("{key:#018x}"))
}
