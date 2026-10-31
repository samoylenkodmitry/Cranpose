use std::rc::Rc;

use cranpose_render_common::Renderer;
use cranpose_ui::{
    PreparedTextLayout, TextLayoutOptions, TextStyle,
    text::{AnnotatedString, prepare_text_layout},
};
use cranpose_ui_graphics::{Rect, Size};
use web_time::Instant;

use crate::{FramePacingMode, RootSurface, ShellApp};

const DEV_OVERLAY_PADDING: f32 = 8.0;
// What a character is taken to measure when the fields to show are picked.
const DEV_OVERLAY_CHAR_WIDTH: f32 = 7.0;
const DEV_OVERLAY_SEPARATOR: &str = " | ";
const DEV_OVERLAY_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Clone, Copy, Debug)]
struct DevOverlayControl {
    bounds: Rect,
    mode: FramePacingMode,
}

// A surface's dev overlay: its line of frame statistics and the frame pacing
// controls in it, built again when the viewport changes or the line is
// older than DEV_OVERLAY_REFRESH_INTERVAL.
#[derive(Default)]
pub(crate) struct DevOverlay {
    controls: Vec<DevOverlayControl>,
    text: String,
    // The line laid out with the app's fonts.
    layout: Option<Rc<PreparedTextLayout>>,
    pub(crate) last_refresh: Option<Instant>,
    viewport: Option<Size>,
}

pub(crate) fn draw_dev_overlay<R: Renderer>(app: &ShellApp, surface: &mut RootSurface<R>) {
    let viewport_size = surface.viewport_size();
    let overlay = &mut surface.dev_overlay;
    overlay.refresh_for_frame_at(app, viewport_size, Instant::now());
    if let Some(layout) = &overlay.layout {
        surface.renderer.draw_dev_overlay(layout, viewport_size);
    }
}

impl DevOverlay {
    pub(crate) fn invalidate(&mut self) {
        self.text.clear();
        self.layout = None;
        self.last_refresh = None;
        self.viewport = None;
    }

    pub(crate) fn control_center(&self, mode: FramePacingMode) -> Option<(f32, f32)> {
        self.controls
            .iter()
            .find(|control| control.mode == mode)
            .map(|control| {
                (
                    control.bounds.x + control.bounds.width * 0.5,
                    control.bounds.y + control.bounds.height * 0.5,
                )
            })
    }

    pub(crate) fn control_at(&self, x: f32, y: f32) -> Option<FramePacingMode> {
        self.controls
            .iter()
            .find(|control| control.bounds.contains(x, y))
            .map(|control| control.mode)
    }

    fn refresh_for_frame_at(&mut self, app: &ShellApp, viewport_size: Size, now: Instant) {
        if !self.needs_refresh(viewport_size, now) {
            return;
        }
        let (text, controls) = self.build_text(app, viewport_size);
        let layout = prepare_text_layout(
            &AnnotatedString::from(text.as_str()),
            &TextStyle::default(),
            TextLayoutOptions::default(),
            None,
        );
        self.controls = place_controls(&layout, viewport_size, controls);
        self.layout = Some(layout);
        self.text = text;
        self.last_refresh = Some(now);
        self.viewport = Some(viewport_size);
    }

    fn needs_refresh(&self, viewport_size: Size, now: Instant) -> bool {
        if self.text.is_empty() || self.viewport != Some(viewport_size) {
            return true;
        }

        self.last_refresh.is_none_or(|last| {
            now.checked_duration_since(last).unwrap_or_default() >= DEV_OVERLAY_REFRESH_INTERVAL
        })
    }

    // The line of statistics that fits `viewport_size`, and the text each
    // frame pacing control takes in it.
    fn build_text(
        &mut self,
        app: &ShellApp,
        viewport_size: Size,
    ) -> (String, Vec<(usize, usize, FramePacingMode)>) {
        let stats = app.fps_monitor.stats();
        // Each field with its rank: a viewport too narrow for the whole line,
        // such as a phone browser's, drops the highest ranks first.
        let fields = [
            (0, format!("{:.0} FPS", stats.fps)),
            (3, format!("avg {:.1}ms", stats.avg_ms)),
            (2, format!("p95 {:.1}ms", stats.p95_ms)),
            (4, format!("max {:.1}ms", stats.max_ms)),
            (1, format!("work {:.1}ms", stats.work_avg_ms)),
            (5, format!("{} recomp/s", stats.recomps_per_second)),
        ];
        let line_len = |max_rank: u8| {
            let (count, chars) = fields
                .iter()
                .filter(|(rank, _)| *rank <= max_rank)
                .fold((0_usize, 0_usize), |(count, chars), (_, field)| {
                    (count + 1, chars + field.len())
                });
            chars + count.saturating_sub(1) * DEV_OVERLAY_SEPARATOR.len()
        };
        let fitting_chars = ((viewport_size.width - DEV_OVERLAY_PADDING * 2.0)
            / DEV_OVERLAY_CHAR_WIDTH)
            .max(0.0) as usize;
        let controls_chars = if app.dev_options.frame_pacing_controls {
            DEV_OVERLAY_SEPARATOR.len() + frame_pacing_controls_len()
        } else {
            0
        };
        let mut max_rank = 5;
        while max_rank > 0 && line_len(max_rank) + controls_chars > fitting_chars {
            max_rank -= 1;
        }
        let mut text = String::with_capacity(line_len(max_rank) + controls_chars);
        for (_, field) in fields.iter().filter(|(rank, _)| *rank <= max_rank) {
            if !text.is_empty() {
                text.push_str(DEV_OVERLAY_SEPARATOR);
            }
            text.push_str(field);
        }

        if !app.dev_options.frame_pacing_controls {
            return (text, Vec::new());
        }

        text.push_str(DEV_OVERLAY_SEPARATOR);
        let mut controls = Vec::with_capacity(FramePacingMode::ALL.len());
        for (index, mode) in FramePacingMode::ALL.into_iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            let start = text.len();
            if mode == app.dev_options.frame_pacing_mode {
                text.push('[');
                text.push_str(mode.label());
                text.push(']');
            } else {
                text.push_str(mode.label());
            }
            controls.push((start, text.len(), mode));
        }

        (text, controls)
    }
}

// The bounds of each control at `controls`' text in the overlay `layout`
// draws in a `viewport_size` window, as the renderer places the overlay.
fn place_controls(
    layout: &PreparedTextLayout,
    viewport_size: Size,
    controls: Vec<(usize, usize, FramePacingMode)>,
) -> Vec<DevOverlayControl> {
    let overlay_x = (viewport_size.width - layout.size().width - DEV_OVERLAY_PADDING * 2.0)
        .max(DEV_OVERLAY_PADDING);
    let text_x = overlay_x + DEV_OVERLAY_PADDING / 2.0;
    let text_y = DEV_OVERLAY_PADDING + DEV_OVERLAY_PADDING / 4.0;
    let caret = |offset| {
        layout
            .layout
            .horizontal_position(offset, cranpose_text::CaretDirection::Primary)
    };
    controls
        .into_iter()
        .map(|(start, end, mode)| DevOverlayControl {
            bounds: Rect {
                x: text_x + caret(start) - 3.0,
                y: text_y - 3.0,
                width: caret(end) - caret(start) + 6.0,
                height: layout.size().height + 6.0,
            },
            mode,
        })
        .collect()
}

/// The length of the frame pacing controls: every mode's label, a space
/// between labels and brackets around the selected one.
fn frame_pacing_controls_len() -> usize {
    let labels: usize = FramePacingMode::ALL
        .iter()
        .map(|mode| mode.label().len())
        .sum();
    labels + FramePacingMode::ALL.len() - 1 + 2
}
