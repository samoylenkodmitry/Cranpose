#[cfg(feature = "cjk")]
fn main() -> std::io::Result<()> {
    use std::{
        env, fs,
        io::{Error, ErrorKind, Write},
        path::PathBuf,
    };

    println!("cargo::rerun-if-changed=build.rs");
    let output = PathBuf::from(
        env::var_os("OUT_DIR")
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Cargo did not set OUT_DIR"))?,
    );
    let mut file = fs::File::create(output.join("CJK.ttf"))?;
    file.write_all(cranpose_fonts_cjk_data_1::DATA)?;
    file.write_all(cranpose_fonts_cjk_data_2::DATA)?;
    file.write_all(cranpose_fonts_cjk_data_3::DATA)?;
    Ok(())
}

#[cfg(not(feature = "cjk"))]
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
}
