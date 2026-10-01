mod shaders_key;

use std::{env, fs, io, path::Path};

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=shaders_key.rs");
    println!("cargo::rerun-if-changed=shaders");
    let key = shaders_key::shaders_key(Path::new("shaders"))?;
    let out = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR unset"))?;
    fs::write(
        Path::new(&out).join("framework_shaders_key.rs"),
        format!("{key:#018x}"),
    )
}
