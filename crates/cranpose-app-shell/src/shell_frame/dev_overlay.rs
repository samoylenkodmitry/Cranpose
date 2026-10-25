use cranpose_render_common::Renderer;
use cranpose_ui_graphics::{Rect, Size};
use web_time::Instant;

use crate::{DevOverlayControl, FramePacingMode, RootSurface, ShellApp};

const DEV_OVERLAY_PADDING: f32 = 8.0;
const DEV_OVERLAY_FONT_SIZE: f32 = 14.0;
const DEV_OVERLAY_CHAR_WIDTH: f32 = 7.0;
const DEV_OVERLAY_SEPARATOR: &str = " | ";
const DEV_OVERLAY_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

pub(super) fn draw_dev_overlay<R: Renderer>(app: &ShellApp, surface: &mut RootSurface<R>) {
    let viewport_size = surface.viewport_size();
    surface.refresh_dev_overlay_text_for_frame_at(app, viewport_size, Instant::now());
    surface
        .renderer
        .draw_dev_overlay(surface.dev_overlay_text.as_str(), viewport_size);
}

impl<R: Renderer> RootSurface<R> {
    fn refresh_dev_overlay_text_for_frame_at(
        &mut self,
        app: &ShellApp,
        viewport_size: Size,
        now: Instant,
    ) {
        if !self.dev_overlay_text_needs_refresh(viewport_size, now) {
            return;
        }
        self.dev_overlay_text = self.build_dev_overlay_text(app, viewport_size);
        self.dev_overlay_last_refresh = Some(now);
        self.dev_overlay_viewport = Some(viewport_size);
    }

    fn dev_overlay_text_needs_refresh(&self, viewport_size: Size, now: Instant) -> bool {
        if self.dev_overlay_text.is_empty() || self.dev_overlay_viewport != Some(viewport_size) {
            return true;
        }

        self.dev_overlay_last_refresh.is_none_or(|last| {
            now.checked_duration_since(last).unwrap_or_default() >= DEV_OVERLAY_REFRESH_INTERVAL
        })
    }

    fn build_dev_overlay_text(&mut self, app: &ShellApp, viewport_size: Size) -> String {
        self.dev_overlay_controls.clear();

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
            return text;
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

        let overlay_width = text.len() as f32 * DEV_OVERLAY_CHAR_WIDTH;
        let overlay_x = (viewport_size.width - overlay_width - DEV_OVERLAY_PADDING * 2.0)
            .max(DEV_OVERLAY_PADDING);
        let overlay_y = DEV_OVERLAY_PADDING;
        let text_x = overlay_x + DEV_OVERLAY_PADDING / 2.0;
        let text_y = overlay_y + DEV_OVERLAY_PADDING / 4.0;
        let text_height = DEV_OVERLAY_FONT_SIZE * 1.4;

        self.dev_overlay_controls = controls
            .into_iter()
            .map(|(start, end, mode)| DevOverlayControl {
                bounds: Rect {
                    x: text_x + start as f32 * DEV_OVERLAY_CHAR_WIDTH - 3.0,
                    y: text_y - 3.0,
                    width: (end - start) as f32 * DEV_OVERLAY_CHAR_WIDTH + 6.0,
                    height: text_height + 6.0,
                },
                mode,
            })
            .collect();

        text
    }
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
