use std::sync::{OnceLock, RwLock};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
}

pub trait TextMeasurer: Send + Sync + 'static {
    fn measure(&self, text: &str) -> TextMetrics;
}

#[derive(Default)]
struct MonospacedTextMeasurer;

impl TextMeasurer for MonospacedTextMeasurer {
    fn measure(&self, text: &str) -> TextMetrics {
        const CHAR_WIDTH: f32 = 8.0;
        const HEIGHT: f32 = 20.0;
        let width = text.chars().count() as f32 * CHAR_WIDTH;
        TextMetrics {
            width,
            height: HEIGHT,
        }
    }
}

fn global_text_measurer() -> &'static RwLock<Box<dyn TextMeasurer>> {
    static TEXT_MEASURER: OnceLock<RwLock<Box<dyn TextMeasurer>>> = OnceLock::new();
    TEXT_MEASURER.get_or_init(|| RwLock::new(Box::new(MonospacedTextMeasurer)))
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    let mut guard = global_text_measurer()
        .write()
        .expect("text measurer lock poisoned");
    *guard = Box::new(measurer);
}

pub fn measure_text(text: &str) -> TextMetrics {
    global_text_measurer()
        .read()
        .expect("text measurer lock poisoned")
        .measure(text)
}
