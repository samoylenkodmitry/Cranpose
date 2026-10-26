//! A frame renderer: each frame draws the texts queued since the last one.

use std::sync::Arc;

/// A device that resources are created on.
pub struct Device {
    label: String,
}

impl Device {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_owned(),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    fn create_buffer(&self, size: usize) -> Vec<u8> {
        vec![0; size]
    }
}

struct GlyphCache {
    glyphs: Vec<char>,
    uploads: usize,
}

impl GlyphCache {
    fn upload(&mut self, device: &Device, text: &str) {
        for glyph in text.chars() {
            if !self.glyphs.contains(&glyph) {
                self.glyphs.push(glyph);
                let _buffer = device.create_buffer(4);
                self.uploads += 1;
            }
        }
    }
}

pub struct Renderer {
    device: Arc<Device>,
    glyphs: GlyphCache,
    pending: Vec<String>,
    drawn: Vec<String>,
    frames: u64,
}

impl Renderer {
    pub fn new(device: Arc<Device>) -> Self {
        Self {
            device,
            glyphs: GlyphCache {
                glyphs: Vec::new(),
                uploads: 0,
            },
            pending: Vec::new(),
            drawn: Vec::new(),
            frames: 0,
        }
    }

    pub fn queue_text(&mut self, text: &str) {
        self.pending.push(text.to_owned());
    }

    /// Draws every queued text and returns how many glyphs the frame uploaded.
    pub fn render_frame(&mut self) -> usize {
        let device = Arc::clone(&self.device);
        let pending = self.pending.clone();
        let before = self.glyphs.uploads;
        for text in &pending {
            self.draw_text(&device, text);
        }
        self.pending.clear();
        self.frames += 1;
        self.glyphs.uploads - before
    }

    fn draw_text(&mut self, device: &Device, text: &str) {
        self.glyphs.upload(device, text);
        self.drawn.push(format!("{}:{}", self.frames, text));
    }

    pub fn drawn(&self) -> &[String] {
        &self.drawn
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }
}
