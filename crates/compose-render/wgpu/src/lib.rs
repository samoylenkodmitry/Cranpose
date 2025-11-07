//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile (Android/iOS).

mod pipeline;
mod render;
mod scene;
mod shaders;

pub use scene::{ClickAction, DrawShape, HitRegion, Scene, TextDraw};

use compose_render_common::{Renderer, RenderScene};
use compose_ui::{set_text_measurer, LayoutTree, TextMeasurer};
use compose_ui_graphics::Size;
use glyphon::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use lru::LruCache;
use render::GpuRenderer;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum WgpuRendererError {
    Layout(String),
    Wgpu(String),
}

/// WGPU-based renderer for GPU-accelerated 2D rendering.
///
/// This renderer supports:
/// - GPU-accelerated shape rendering (rectangles, rounded rectangles)
/// - Gradients (solid, linear, radial)
/// - GPU text rendering via glyphon
/// - Cross-platform support (Desktop, Web, Mobile)
pub struct WgpuRenderer {
    scene: Scene,
    gpu_renderer: Option<GpuRenderer>,
    font_system: Arc<Mutex<FontSystem>>,
}

impl WgpuRenderer {
    /// Create a new WGPU renderer without GPU resources.
    /// Call `init_gpu` before rendering.
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();

        // Load Roboto font into the system
        let font_data = include_bytes!("../../../../apps/desktop-demo/assets/Roboto-Light.ttf");
        font_system.db_mut().load_font_data(font_data.to_vec());

        let font_system = Arc::new(Mutex::new(font_system));
        let text_measurer = WgpuTextMeasurer::new(font_system.clone());
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
        }
    }

    /// Initialize GPU resources with a WGPU device and queue.
    pub fn init_gpu(
        &mut self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) {
        self.gpu_renderer = Some(GpuRenderer::new(
            device,
            queue,
            surface_format,
            self.font_system.clone(),
        ));
    }

    /// Render the scene to a texture view.
    pub fn render(&mut self, view: &wgpu::TextureView, width: u32, height: u32) -> Result<(), WgpuRendererError> {
        if let Some(gpu_renderer) = &mut self.gpu_renderer {
            gpu_renderer
                .render(view, &self.scene.shapes, &self.scene.texts, width, height)
                .map_err(|e| WgpuRendererError::Wgpu(e))
        } else {
            Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized. Call init_gpu() first.".to_string(),
            ))
        }
    }

    /// Get access to the WGPU device (for surface configuration).
    pub fn device(&self) -> &wgpu::Device {
        self.gpu_renderer
            .as_ref()
            .map(|r| &*r.device)
            .expect("GPU renderer not initialized")
    }
}

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for WgpuRenderer {
    type Scene = Scene;
    type Error = WgpuRendererError;

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(&mut self, layout_tree: &LayoutTree, _viewport: Size) -> Result<(), Self::Error> {
        self.scene.clear();
        pipeline::render_layout_tree(layout_tree.root(), &mut self.scene);
        Ok(())
    }
}

// Text measurer implementation for WGPU

/// Cached measurement buffer to avoid redundant text shaping
struct MeasurementBuffer {
    buffer: Buffer,
    text: String,
    font_size: f32,
}

impl MeasurementBuffer {
    fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        font_size: f32,
        attrs: Attrs,
    ) -> bool {
        // Check if anything changed that requires reshaping
        if self.text == text && self.font_size == font_size {
            return false; // No reshaping needed!
        }

        // Something changed, need to reshape
        let metrics = Metrics::new(font_size, font_size * 1.4);
        self.buffer.set_metrics(font_system, metrics);
        self.buffer.set_text(font_system, text, attrs, Shaping::Advanced);
        self.buffer.shape_until_scroll(font_system);

        // Update cached values
        self.text.clear();
        self.text.push_str(text);
        self.font_size = font_size;

        true
    }
}

#[derive(Clone)]
struct WgpuTextMeasurer {
    font_system: Arc<Mutex<FontSystem>>,
    cache: Arc<Mutex<LruCache<(String, i32), Size>>>,
    buffer_cache: Arc<Mutex<LruCache<(String, i32), Box<MeasurementBuffer>>>>,
}

impl WgpuTextMeasurer {
    fn new(font_system: Arc<Mutex<FontSystem>>) -> Self {
        Self {
            font_system,
            cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(64).unwrap()))),
            buffer_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(64).unwrap()))),
        }
    }
}

impl TextMeasurer for WgpuTextMeasurer {
    fn measure(&self, text: &str) -> compose_ui::TextMetrics {
        let font_size = 14.0; // Default font size
        let key = (text.to_string(), (font_size * 100.0) as i32);

        // Check size cache first (fastest path)
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(size) = cache.get(&key) {
                return compose_ui::TextMetrics {
                    width: size.width,
                    height: size.height,
                };
            }
        }

        // Need to measure - check buffer cache
        let mut font_system = self.font_system.lock().unwrap();
        let mut buffer_cache = self.buffer_cache.lock().unwrap();

        let buffer = if let Some(cached) = buffer_cache.get_mut(&key) {
            // Buffer cache hit - use ensure() to only reshape if needed
            cached.ensure(&mut font_system, text, font_size, Attrs::new());
            &cached.buffer
        } else {
            // Buffer cache miss - create new buffer
            let mut new_buffer = Buffer::new(&mut font_system, Metrics::new(font_size, font_size * 1.4));
            new_buffer.set_size(&mut font_system, f32::MAX, f32::MAX);
            new_buffer.set_text(
                &mut font_system,
                text,
                Attrs::new(),
                Shaping::Advanced,
            );
            new_buffer.shape_until_scroll(&mut font_system);

            let cached = Box::new(MeasurementBuffer {
                buffer: new_buffer,
                text: text.to_string(),
                font_size,
            });
            buffer_cache.put(key.clone(), cached);
            &buffer_cache.get(&key).unwrap().buffer
        };

        // Measure the shaped buffer
        let mut max_width = 0.0f32;
        let mut total_height = 0.0f32;

        for _line in buffer.lines.iter() {
            let layout_runs = buffer.layout_runs();
            for run in layout_runs {
                max_width = max_width.max(run.line_w);
                break; // Just get the first run's width
            }
            total_height += font_size * 1.4;
        }

        let size = Size {
            width: max_width,
            height: total_height,
        };

        // Cache the size result
        let mut cache = self.cache.lock().unwrap();
        cache.put(key, size);

        compose_ui::TextMetrics {
            width: size.width,
            height: size.height,
        }
    }
}
