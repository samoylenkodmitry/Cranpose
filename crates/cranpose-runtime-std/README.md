# Cranpose Runtime Std

Platform-agnostic standard library implementations for Cranpose runtime interfaces.

## When to Use

This crate is used by application shells (like `cranpose-app-shell` or custom embedding layers) to provide standard clock and scheduling capabilities to the `cranpose-core` runtime. You typically do not need to interact with this crate unless you are:
-   Porting Cranpose to a new platform that supports Rust `std`.
-   Implementing a custom `AppLauncher` or event loop.

## Key Concepts

-   **FrameClock**: Abstraction for synchronizing updates with the display refresh rate (VSync). This crate provides implementations based on `std::time` or platform-specific APIs where applicable.
-   **RuntimeScheduler**: Interface for scheduling future tasks. This implementation bridges the core runtime with standard async executors or thread pools.
-   **MonotonicClock**: Provides high-precision , non-decreasing time measurements critical for animations and input event timestamps.

## Architecture

This crate acts as a bridge between the abstract definitions in `cranpose-core` and the concrete operating system capabilities provided by the Rust standard library.
