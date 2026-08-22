//! Built-in vector icon set (24×24 viewBox path data, Material Design icons,
//! Apache-2.0), rasterized through [`cranpose_ui_graphics::VectorPath`] and
//! tinted at draw time.

use cranpose_core::rememberKeyed;
use cranpose_macros::composable;
use cranpose_ui::widgets::{Box, BoxSpec};
use cranpose_ui::{Modifier, Size};
use cranpose_ui_graphics::{Brush, Color, VectorPath};

pub const CHEVRON_LEFT: &str = "M15.41 7.41L14 6l-6 6 6 6 1.41-1.41L10.83 12z";
pub const CHEVRON_RIGHT: &str = "M10 6L8.59 7.41 13.17 12l-4.58 4.59L10 18l6-6z";
pub const CHEVRON_DOWN: &str = "M16.59 8.59L12 13.17 7.41 8.59 6 10l6 6 6-6z";
pub const CHEVRON_UP: &str = "M12 8l-6 6 1.41 1.41L12 10.83l4.59 4.58L18 14z";
pub const ARROW_BACK: &str = "M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z";
pub const SEARCH: &str = "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z";
pub const SETTINGS: &str = "M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58c.18-.14.23-.41.12-.61l-1.92-3.32c-.12-.22-.37-.29-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94L14.4 2.81c-.04-.24-.24-.41-.48-.41h-3.84c-.24 0-.43.17-.47.41L9.25 5.35c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58c-.18.14-.23.41-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z";
pub const MORE_VERT: &str = "M12 8c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm0 2c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm0 6c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z";
pub const MORE_HORIZ: &str = "M6 10c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm12 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm-6 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z";
pub const PLUS: &str = "M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6z";
pub const SHARE: &str = "M9 16h6v-6h4l-7-7-7 7h4zm-4 2h14v2H5z";
pub const TRASH: &str =
    "M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z";
pub const DOCUMENT: &str = "M14 2H6c-1.1 0-1.99.9-1.99 2L4 20c0 1.1.89 2 1.99 2H18c1.1 0 2-.9 2-2V8l-6-6zm2 16H8v-2h8v2zm0-4H8v-2h8v2zm-3-5V3.5L18.5 9H13z";
pub const CAMERA: &str = "M12 12m-3.2 0a3.2 3.2 0 1 0 6.4 0a3.2 3.2 0 1 0-6.4 0M9 2L7.17 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2h-3.17L15 2H9zm3 15c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5z";
pub const TRANSLATE: &str = "M12.87 15.07l-2.54-2.51.03-.03c1.74-1.94 2.98-4.17 3.71-6.53H17V4h-7V2H8v2H1v2h11.17C11.5 7.92 10.44 9.75 9 10.35 7.91 9.12 7.07 7.7 6.47 6h-2c.73 2.21 1.94 4.27 3.54 6.06l-5.09 5.02 1.42 1.42L9 13.83l2.9 2.9.97-1.66zM18.5 10h-2L12 22h2l1.12-3h4.75L21 22h2l-4.5-12zm-2.62 7 1.62-4.33L19.12 17h-3.24z";
pub const GROUP: &str = "M16 11c1.66 0 3-1.34 3-3s-1.34-3-3-3c-.32 0-.63.05-.91.14.57.8.91 1.79.91 2.86s-.34 2.06-.91 2.86c.28.09.59.14.91.14zM8 11c1.66 0 3-1.34 3-3S9.66 5 8 5 5 6.34 5 8s1.34 3 3 3zm0 2c-2.33 0-7 1.17-7 3.5V19h14v-2.5C15 14.17 10.33 13 8 13zm8 0c-.29 0-.62.02-.97.05C16.19 13.89 17 15.02 17 16.5V19h6v-2.5c0-2.33-4.67-3.5-7-3.5z";
pub const FOLDER: &str =
    "M10 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z";
pub const CHECK: &str = "M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z";
pub const CLOSE: &str = "M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z";
pub const STAR: &str =
    "M12 17.27L18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z";
pub const BOOKMARK: &str = "M17 3H7c-1.1 0-2 .9-2 2v16l7-3 7 3V5c0-1.1-.9-2-2-2z";
pub const EYE: &str = "M12 4.5C7 4.5 2.73 7.61 1 12c1.73 4.39 6 7.5 11 7.5s9.27-3.11 11-7.5c-1.73-4.39-6-7.5-11-7.5zM12 17c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5zm0-8c-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3-1.34-3-3-3z";
pub const HOME: &str = "M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z";
pub const APPLE: &str = "M17.05 20.28c-.98.95-2.05.8-3.08.35-1.09-.47-2.09-.48-3.24 0-1.44.62-2.2.44-3.06-.35C2.79 15.25 3.51 7.59 8.57 7.31c1.34-.07 2.29.74 3.08.74.76 0 2.21-.91 3.72-.77.63.03 2.4.25 3.54 1.92-3.27 1.79-2.76 6.08.57 7.41-.67 1.76-1.54 3.5-2.89 4.87M12.03 7.25C11.88 4.62 13.99 2.45 16.44 2.25c.34 3.04-2.76 5.3-4.41 5";
pub const ACCOUNT_CIRCLE: &str = "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 3c1.66 0 3 1.34 3 3s-1.34 3-3 3-3-1.34-3-3 1.34-3 3-3zm0 14.2c-2.5 0-4.71-1.28-6-3.22.03-1.99 4-3.08 6-3.08 1.99 0 5.97 1.09 6 3.08-1.29 1.94-3.5 3.22-6 3.22z";
pub const SHOPPING_BAG: &str = "M5 8h14c1.1 0 2 .9 2 2v9c0 1.1-.9 2-2 2H5c-1.1 0-2-.9-2-2v-9c0-1.1.9-2 2-2zM5 5h14v2H5zM7 2h10v2H7z";
pub const LAYERS: &str = "M12 2 2 7l10 5 10-5-10-5zM2 12l10 5 10-5v3l-10 5-10-5v-3z";
pub const ROCKET: &str = "M13.5 2C17 3 20 6.5 20 10l-5.2 5.2-6-6L13.5 2zM8.8 9.2 4 10.5 2 14l5.5-.5 1.3-4.3zM14.8 15.2 13.5 22l3.5-2 1.3-4.8h-3.5zM6 17c-2 0-3 2-3 4 2 0 4-1 4-3l-1-1zM15.5 6.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z";
pub const JOYSTICK: &str = "M11 2h2v7h-2V2zm1 6c4.42 0 8 2.24 8 5s-3.58 5-8 5-8-2.24-8-5 3.58-5 8-5zm-5 5h2v-2h2v2h2v2h-2v2H9v-2H7v-2zm8-1h2v2h-2v-2zM5 18h14l2 4H3l2-4z";
pub const GRID: &str =
    "M4 11h5V5H4v6zm0 7h5v-6H4v6zm6 0h5v-6h-5v6zm6 0h5v-6h-5v6zm-6-7h5V5h-5v6zm6-6v6h5V5h-5z";
pub const LIST: &str =
    "M4 5h3v3H4V5zm5 0h11v3H9V5zM4 10.5h3v3H4v-3zm5 0h11v3H9v-3zM4 16h3v3H4v-3zm5 0h11v3H9v-3z";
pub const GRID_OUTLINE: &str = "M3 3h8v8H3V3zm2 2v4h4V5H5zm8-2h8v8h-8V3zm2 2v4h4V5h-4zM3 13h8v8H3v-8zm2 2v4h4v-4H5zm8-2h8v8h-8v-8zm2 2v4h4v-4h-4z";
pub const LIST_OUTLINE: &str = "M4 5h2v2H4V5zm5 .25h11v1.5H9v-1.5zM4 11h2v2H4v-2zm5 .25h11v1.5H9v-1.5zM4 17h2v2H4v-2zm5 .25h11v1.5H9v-1.5z";
pub const SCHEDULE: &str = "M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zM12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8zm.5-13H11v6l5.25 3.15.75-1.23-4.5-2.67z";
pub const DOWNLOAD: &str = "M19 9h-4V3H9v6H5l7 7 7-7zM5 18v2h14v-2H5z";
pub const EDIT: &str = "M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04c.39-.39.39-1.02 0-1.41l-2.34-2.34c-.39-.39-1.02-.39-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z";
pub const GEAR: &str = SETTINGS;
pub const FILTER: &str = "M10 18h4v-2h-4v2zM3 6v2h18V6H3zm3 7h12v-2H6v2z";

/// The icon path data's design grid.
const ICON_VIEW_BOX: f32 = 24.0;

/// A tinted vector icon. `path` is 24×24 viewBox path data (one of this
/// module's constants or any compatible data); `size` is the rendered square
/// in dp.
#[composable]
#[allow(non_snake_case)]
pub fn Icon(path: &'static str, size: f32, color: Color) {
    // The parse cache is keyed on the path: a position-only remember served
    // a STALE glyph when a reused slot changed icons (the accordion's
    // "Sections" row inherited the collapsed row's chevron). Scaling happens
    // at draw time so a size change on recomposition is honored.
    let parsed = rememberKeyed(path, |path| VectorPath::parse(path).ok());
    Box(
        Modifier::empty()
            .size(Size::new(size, size))
            .draw_behind(move |scope| {
                if let Some(path) = &parsed {
                    let scaled = path.scaled(size / ICON_VIEW_BOX);
                    scope.draw_vector_path(&scaled, Brush::solid(color));
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shopping_bag_has_the_stacked_tab_icon_silhouette() {
        let bounds = VectorPath::parse(SHOPPING_BAG)
            .expect("shopping bag path")
            .bounds();
        assert!((bounds.width - 18.0).abs() < 1.0e-4);
        assert!((bounds.height - 19.0).abs() < 1.0e-4);
    }
}
