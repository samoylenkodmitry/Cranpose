# Cranpose capabilities

Declares the services and hardware requirements an application uses. A build
script calls `declare(&[Use::camera(...), Use::notifications()]).emit()` to
write platform permission and usage-description metadata, plus a Rust
capabilities value the application can pass to `AppLauncher`.

The `cranpose-plist` binary writes Apple property-list entries from capability
declarations. See the crate-level API documentation for the full set of
services, reasons and hardware demands.
