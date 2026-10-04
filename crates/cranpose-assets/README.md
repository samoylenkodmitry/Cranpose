# Cranpose Assets

`cranpose-assets` loads local application files through a synchronous asset manager. The manager searches registered roots in order, caches each file as shared bytes, and converts UTF-8 files to `String` values.

Add `cranpose-assets` to the app's dependencies. Store files under an app asset directory such as `assets/`, then create a manager with the directory as its root:

```rust
use cranpose_assets::AssetManager;

fn read_manifest() -> Result<String, cranpose_assets::AssetError> {
    let assets = AssetManager::with_root("assets");
    assets.load_string("manifest.txt")
}
```

`load_bytes` returns `Arc<[u8]>`, which lets callers share cached data. `add_root` adds another lookup directory; earlier roots take priority. `clear_cache` releases cached entries. Use relative asset names whose path components stay inside a registered root. The manager reads filesystem assets synchronously. Applications can use [`cranpose-services`](https://docs.rs/cranpose-services/latest/cranpose_services/) content APIs for network and platform document access.

## Links

- [API documentation](https://docs.rs/cranpose-assets/latest/cranpose_assets/)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-assets)
- [Cranpose guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md)
