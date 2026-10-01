use std::{fs, path::Path};

#[path = "../../shaders_key.rs"]
mod shaders_key;

use self::shaders_key::shaders_key;
use super::SOURCES_KEY;

#[test]
fn the_sources_key_is_the_key_of_the_shaders_on_disk() {
    let shaders = Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
    assert_eq!(
        SOURCES_KEY,
        shaders_key(&shaders).expect("read the framework shaders")
    );
}

#[test]
fn the_key_follows_every_shader_and_nothing_else() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-output")
        .join(format!("shaders-key-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create shader directory");
    fs::write(dir.join("a.wgsl"), "fn a() {}").expect("write a");
    fs::write(dir.join("b.wgsl"), "fn b() {}").expect("write b");
    let key = |what: &str| shaders_key(&dir).expect(what);
    let original = key("key the shaders");

    fs::write(dir.join("notes.txt"), "not a shader").expect("write notes");
    assert_eq!(key("key beside notes"), original, "only shaders count");

    fs::write(dir.join("b.wgsl"), "fn b() { }").expect("edit b");
    let edited = key("key an edited shader");
    assert_ne!(edited, original, "an edited shader");

    fs::rename(dir.join("b.wgsl"), dir.join("c.wgsl")).expect("rename b");
    let renamed = key("key a renamed shader");
    assert_ne!(renamed, edited, "a renamed shader");

    fs::write(dir.join("a.wgsl"), "fn a() {}fn b() { }").expect("join a and c");
    fs::write(dir.join("c.wgsl"), "").expect("empty c");
    assert_ne!(
        key("key moved text"),
        renamed,
        "text moved across a file boundary"
    );
    fs::remove_dir_all(&dir).expect("remove shader directory");
}
