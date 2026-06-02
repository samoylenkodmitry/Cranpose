# iOS Demo

Cranpose does not currently ship an iOS backend.

The `ios` cargo feature is reserved for a real UIKit/CAMetalLayer platform crate. It must not alias desktop behavior, and this demo intentionally fails until the backend provides a native surface, CADisplayLink frame driver, lifecycle bridge, input bridge, safe-area handling, and density updates.
