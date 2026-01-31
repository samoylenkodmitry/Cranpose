# Cranpose Render WGPU

A GPU-accelerated rendering backend for Cranpose, powered by `wgpu`.

## When to Use

This is the default renderer for Cranpose. It provides high-performance rendering on Linux (Vulkan/GL), macOS (Metal), Windows (DX12/Vulkan), and Web (WebGL2/WebGPU). It supports advanced features like anti-aliased shapes and hardware-accelerated text rendering.

## Key Concepts

-   **WGPU**: The underlying graphics abstraction layer.
-   **Glyphon**: Used for efficient, high-quality text layout and rasterization.
-   **Batching**: The renderer automatically batches draw calls to minimize GPU overhead.

## Configuration

This renderer is enabled by default via the `renderer-wgpu` feature in the `cranpose` crate. No manual configuration is typically required.
