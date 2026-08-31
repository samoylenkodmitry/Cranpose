use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("desktop demo should live under workspace/apps")
        .to_path_buf()
}

fn unique_test_root(workspace_root: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should follow the Unix epoch")
        .as_nanos();
    workspace_root.join(format!(
        "target/test-output/web-release-bundle-{}-{nonce}",
        std::process::id()
    ))
}

fn write_fixture(package_dir: &Path, wasm: &[u8]) {
    fs::create_dir_all(package_dir).expect("fixture package directory should be created");
    fs::write(
        package_dir.join("desktop_app.js"),
        "export default async function init() {}\n",
    )
    .expect("fixture JavaScript should be written");
    fs::write(package_dir.join("desktop_app_bg.wasm"), wasm)
        .expect("fixture WASM should be written");
}

fn package_site(script: &Path, output: &Path, package_dir: &Path, index: &Path) -> Value {
    let result = Command::new(script)
        .args([output, package_dir, index])
        .output()
        .expect("web packaging script should run");
    assert!(
        result.status.success(),
        "web packaging failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let manifest = fs::read(output.join("asset-manifest.json"))
        .expect("packaged site should contain an asset manifest");
    serde_json::from_slice(&manifest).expect("asset manifest should be valid JSON")
}

#[test]
fn release_assets_are_content_addressed_as_one_consistent_bundle() {
    let workspace_root = workspace_root();
    let test_root = unique_test_root(&workspace_root);
    let package_dir = test_root.join("pkg");
    let index = test_root.join("index.html");
    fs::create_dir_all(&test_root).expect("test output root should be created");
    fs::write(&index, "<!doctype html><title>fixture</title>\n")
        .expect("fixture index should be written");

    let script = workspace_root.join("apps/desktop-demo/package-web.sh");
    write_fixture(&package_dir, b"wasm-one");
    let first_output = test_root.join("site-one");
    let first_manifest = package_site(&script, &first_output, &package_dir, &index);

    write_fixture(&package_dir, b"wasm-two");
    let second_output = test_root.join("site-two");
    let second_manifest = package_site(&script, &second_output, &package_dir, &index);

    let first_module = first_manifest["module"]
        .as_str()
        .expect("manifest module should be a string");
    let second_module = second_manifest["module"]
        .as_str()
        .expect("manifest module should be a string");
    assert_ne!(
        first_module, second_module,
        "changing either half of a release bundle must change its asset URL"
    );
    assert!(
        first_output.join(first_module).is_file(),
        "first manifest must address its matching JavaScript"
    );
    assert!(
        second_output.join(second_module).is_file(),
        "second manifest must address its matching JavaScript"
    );

    for (output, manifest) in [
        (&first_output, &first_manifest),
        (&second_output, &second_manifest),
    ] {
        let module = Path::new(
            manifest["module"]
                .as_str()
                .expect("manifest module should be a string"),
        );
        let wasm = Path::new(
            manifest["wasm"]
                .as_str()
                .expect("manifest WASM should be a string"),
        );
        assert_eq!(
            module.parent(),
            wasm.parent(),
            "JavaScript and WASM must be published in one content-addressed directory"
        );
        assert!(output.join(wasm).is_file(), "manifest WASM must exist");
    }
}

#[test]
fn browser_boot_surface_uses_the_content_addressed_manifest() {
    let workspace_root = workspace_root();
    let index = fs::read_to_string(workspace_root.join("apps/desktop-demo/index.html"))
        .expect("desktop demo index should be readable");
    let workflow = fs::read_to_string(workspace_root.join(".github/workflows/deploy-pages.yml"))
        .expect("Pages workflow should be readable");

    for required in [
        "id=\"boot-surface\"",
        "role=\"status\"",
        "asset-manifest.json",
        "cache: 'no-store'",
        "Date.now()",
        "await import(moduleUrl.href)",
        "errorMessage.textContent",
    ] {
        assert!(
            index.contains(required),
            "browser boot surface must contain `{required}`"
        );
    }
    for removed_marketing_shell in [
        "Jetpack Compose-inspired UI framework for Rust",
        "Powered by Rust",
        "Full demo with interactive examples",
        "class=\"links\"",
        "errorDiv.innerHTML",
    ] {
        assert!(
            !index.contains(removed_marketing_shell),
            "full-screen WASM boot must not retain `{removed_marketing_shell}`"
        );
    }
    assert!(
        workflow.contains("apps/desktop-demo/package-web.sh")
            && workflow.contains("CRANPOSE_PAGES_SITE_ROOT")
            && workflow.contains("path: ${{ env.CRANPOSE_PAGES_SITE_ROOT }}"),
        "Pages must upload the content-addressed packaged site from a unique runner path"
    );
}
