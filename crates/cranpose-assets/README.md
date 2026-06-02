# Cranpose Assets

Small synchronous asset loading and caching utilities.

`AssetManager` resolves relative asset paths under one or more registered roots,
rejects absolute paths and `..` traversal, caches loaded byte buffers, and can
decode UTF-8 text assets.

```rust
use cranpose_assets::AssetManager;

let manager = AssetManager::with_root("assets");
let icon_bytes = manager.load_bytes("icons/app.rgba")?;
let manifest = manager.load_string("manifest.txt")?;
# Ok::<(), cranpose_assets::AssetError>(())
```
