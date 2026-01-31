# Cranpose App Shell

The runtime orchestration layer for Cranpose applications.

## When to Use

This is an internal crate that acts as the glue between the platform shell (windowing), the runtime (scheduler/clock), and the UI framework. It manages the main application loop. Advanced users might inspect this crate to understand the lifecycle of a frame or how input events flow from the OS to the composition tree.

## Key Concepts

-   **App Lifecycle**: Initialization, Loop execution, and Shutdown.
-   **Frame Scheduling**: Coordinates `measure`, `layout`, and `draw` phases based on vsync signals and state invalidations.
-   **Input Dispatch**: Routes raw window events to the `FocusManager` and `PointerInput` subsystems.

## Usage

This crate is typically instantiated and managed by the `AppLauncher` in the `cranpose` crate.
