use super::*;

fn test_asset_root() -> PathBuf {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/cranpose-assets");
    std::fs::create_dir_all(&root).expect("create asset test output directory");
    root
}

#[test]
fn an_earlier_root_wins_over_one_added_later() {
    let base = test_asset_root().join("root-order");
    let first = base.join("first");
    let second = base.join("second");
    std::fs::create_dir_all(&first).expect("first root");
    std::fs::create_dir_all(&second).expect("second root");
    std::fs::write(first.join("shared.txt"), b"first").expect("write first");
    std::fs::write(second.join("shared.txt"), b"second").expect("write second");
    std::fs::write(second.join("only.txt"), b"only").expect("write only");

    let mut assets = AssetManager::with_root(&first);
    assets.add_root(&second);
    assert_eq!(assets.roots(), &[first, second.clone()]);

    assert_eq!(
        assets.load_bytes("shared.txt").expect("shared").as_ref(),
        b"first"
    );
    assert_eq!(
        assets.load_bytes("only.txt").expect("only").as_ref(),
        b"only"
    );
}

#[test]
fn load_bytes_reads_and_caches_relative_asset() {
    let root = test_asset_root();
    std::fs::write(root.join("sample.bin"), [1u8, 2, 3]).expect("write test asset");
    let manager = AssetManager::with_root(&root);

    let first = manager.load_bytes("sample.bin").expect("load asset");
    let second = manager
        .load_bytes("./sample.bin")
        .expect("load cached asset");

    assert_eq!(&*first, &[1, 2, 3]);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(manager.cached_asset_count(), 1);
}

#[test]
fn load_string_returns_utf8_content() {
    let root = test_asset_root();
    std::fs::write(root.join("message.txt"), "hello cranpose").expect("write text asset");
    let manager = AssetManager::with_root(&root);

    assert_eq!(
        manager.load_string("message.txt").expect("load text asset"),
        "hello cranpose"
    );
}

#[test]
fn load_rejects_paths_that_escape_root() {
    let manager = AssetManager::with_root(test_asset_root());

    assert!(matches!(
        manager.load_bytes("../outside.bin"),
        Err(AssetError::EscapesRoot { .. })
    ));
}

#[test]
fn load_reports_missing_asset_with_roots() {
    let root = test_asset_root();
    let manager = AssetManager::with_root(&root);

    match manager.load_bytes("missing.bin") {
        Err(AssetError::NotFound { path, roots }) => {
            assert_eq!(path, PathBuf::from("missing.bin"));
            assert_eq!(roots, vec![root]);
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[test]
fn clear_cache_removes_cached_assets() {
    let root = test_asset_root();
    std::fs::write(root.join("clear.bin"), [9u8]).expect("write test asset");
    let manager = AssetManager::with_root(&root);

    manager.load_bytes("clear.bin").expect("load asset");
    assert_eq!(manager.cached_asset_count(), 1);
    manager.clear_cache();
    assert_eq!(manager.cached_asset_count(), 0);
}
