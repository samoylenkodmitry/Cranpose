use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

// A file that two include macros read sits in the binary twice when its
// readers land in different codegen units. The web demo carried the torus
// of the room and of the explainer's curtain twice that way: 43 KB.
// Shaders stay out: each one is a text of its own that composes the shared
// parts of other files.
#[test]
fn no_asset_is_embedded_by_two_include_macros() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut readers = BTreeMap::new();
    collect(&root.join("src"), &mut readers);
    let twice: Vec<String> = readers
        .iter()
        .filter(|(_, sources)| sources.len() > 1)
        .map(|(asset, sources)| {
            let asset = asset.strip_prefix(root).unwrap_or(asset);
            format!("{} is read by {}", asset.display(), sources.join(", "))
        })
        .collect();
    assert!(
        twice.is_empty(),
        "an asset is embedded twice; read it from one static:\n{}",
        twice.join("\n")
    );
}

fn collect(directory: &Path, readers: &mut BTreeMap<PathBuf, Vec<String>>) {
    let entries = fs::read_dir(directory).expect("the demo's sources can be listed");
    for entry in entries {
        let path = entry.expect("a source directory entry reads").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "tests") {
                collect(&path, readers);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).expect("a demo source reads");
            for (line, text) in source.lines().enumerate() {
                if text.trim_start().starts_with("//") {
                    continue;
                }
                for asset in included_assets(text, &path) {
                    readers.entry(asset).or_default().push(format!(
                        "{}:{}",
                        path.display(),
                        line + 1
                    ));
                }
            }
        }
    }
}

// The files that `include_str!` and `include_bytes!` read on one line of a
// source, except shaders.
fn included_assets(line: &str, source: &Path) -> Vec<PathBuf> {
    let directory = source.parent().expect("a source has a directory");
    ["include_str!(\"", "include_bytes!(\""]
        .into_iter()
        .flat_map(|macro_call| line.split(macro_call).skip(1))
        .filter_map(|rest| rest.split_once('"'))
        .map(|(relative, _)| Path::new(relative))
        .filter(|relative| {
            relative
                .extension()
                .is_none_or(|extension| extension != "wgsl")
        })
        .map(|relative| {
            fs::canonicalize(directory.join(relative)).expect("an included asset exists")
        })
        .collect()
}
