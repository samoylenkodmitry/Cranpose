# Cranpose Assets

Resource loading and management utilities.

## When to Use

Use this crate to manage static assets for your application. It abstracts over filesystem paths and bundle locations, providing a unified way to load images and fonts across different platforms (Desktop, Android, Web).

## Key Concepts

-   **Asset Loader**: Handles asynchronous loading of resources.
-   **Caching**: Automatically caches loaded assets in memory to prevent redundant I/O operations.
-   **Platform Abstraction**: Handles path differences between an Android APK, a Web URL, and a local filesystem.

## Example

```rust
use cranpose::prelude::*;

#[composable]
fn Logo() {
    // Loads an image from the assets directory.
    // Ensure "assets/logo.png" exists in your project root or platform-specific asset folder.
    let image = load_image("assets/logo.png");
    
    Image(
        painter = image,
        content_description = "App Logo",
        modifier = Modifier.size(50.dp)
    );
}
```
