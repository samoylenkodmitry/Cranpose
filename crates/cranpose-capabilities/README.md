# Cranpose Capabilities

`cranpose-capabilities` declares the platform services, hardware demands, and file types an app uses. One Rust declaration feeds Android permissions and intent filters, Apple usage descriptions, and a generated `Capabilities` value for the runtime.

## Declare app capabilities

Add `cranpose-capabilities` as a build dependency. In app code, call `cranpose::app_capabilities!()` to include the generated constant. Then call `emit` from `build.rs`:

```rust,no_run
use cranpose_capabilities::{Use, declare};

fn main() {
    declare(&[
        Use::camera("Reads a receipt with the camera."),
        Use::notifications(),
    ])
    .opening(&["image/*", "application/pdf"])
    .emit();
}
```

`emit` writes `OUT_DIR/cranpose_capabilities.rs` with a `CAPABILITIES` constant. The Cranpose macro includes the file; pass the constant to `AppLauncher::with_capabilities`:

```rust,ignore
cranpose::app_capabilities!();

fn launcher() -> cranpose::AppLauncher {
    cranpose::AppLauncher::new().with_capabilities(&CAPABILITIES)
}
```

The build also writes package permission XML and Apple usage plist data under `<workspace>/target/cranpose`. Set `CRANPOSE_CAPABILITIES_DIR` when a platform build reads declarations from a separate output directory. The `cranpose-plist` binary prints Apple property-list entries from a capability declaration.

`Use` lists services such as camera, location, microphone, notifications, media, billing, overlay, and network. Apple camera, photo library, microphone, and location entries take a reason for the person. `Demand` marks hardware the app requires; other services stay optional for runtime checks.

## Links

- [API documentation](https://docs.rs/cranpose-capabilities/latest/cranpose_capabilities/)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-capabilities)
