# Cranpose desktop platform

This internal platform crate connects the Cranpose application shell to
`winit` on Linux, macOS and Windows. The crate owns the native event loop,
window and surface lifecycle, and translates window and input events. Applications
normally enable `desktop`, `desktop-x11` or `desktop-wayland` on `cranpose`.

Application code uses `AppLauncher`; the desktop launcher accepts a title,
logical-pixel size and root composable closure:

```rust,ignore
use cranpose::AppLauncher;

fn main() {
    AppLauncher::new()
        .with_title("Desktop App")
        .with_size(800, 600)
        .run(|| {
            MyApp();
        });
}
```
