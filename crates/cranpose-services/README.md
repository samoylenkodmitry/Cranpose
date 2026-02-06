# Cranpose Services

Multiplatform service abstractions for Cranpose applications.

## When to Use

This crate provides cross-platform interfaces and default implementations for:

- HTTP text fetching
- Opening external URIs

Applications can consume these services through CompositionLocals and override them in tests.

## Architecture

- **Interfaces**: `HttpClient`, `UriHandler`
- **CompositionLocals**: `local_http_client()`, `local_uri_handler()`
- **Default implementations**:
  - Desktop: `reqwest` for HTTP and `open` for URIs
  - Web: browser `fetch` and `window.open`
  - Android URI opening currently returns `UnsupportedPlatform`
