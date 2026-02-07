//! Sprite region definitions and fixed layout coordinates for Winamp 2.x skins.

#![allow(dead_code)]

use cranpose_ui_graphics::Rect;

/// Source rectangle in a sprite sheet: `(x, y, width, height)`.
pub type SpriteRect = (f32, f32, f32, f32);

/// Converts a sprite tuple into a [`Rect`].
pub fn to_rect(rect: SpriteRect) -> Rect {
    Rect {
        x: rect.0,
        y: rect.1,
        width: rect.2,
        height: rect.3,
    }
}

// Main window geometry
pub const MAIN_WIDTH: f32 = 275.0;
pub const MAIN_HEIGHT: f32 = 116.0;
pub const MAIN_WINDOW: SpriteRect = (0.0, 0.0, MAIN_WIDTH, MAIN_HEIGHT);
pub const TITLE_DRAG_AREA: SpriteRect = (0.0, 0.0, 275.0, 14.0);

// Source slices: TITLEBAR.BMP
pub const MAIN_TITLE_BAR: SpriteRect = (27.0, 15.0, 275.0, 14.0);
pub const MAIN_TITLE_BAR_SELECTED: SpriteRect = (27.0, 0.0, 275.0, 14.0);
pub const MAIN_OPTIONS_BUTTON: SpriteRect = (0.0, 0.0, 9.0, 9.0);
pub const MAIN_OPTIONS_BUTTON_SELECTED: SpriteRect = (0.0, 9.0, 9.0, 9.0);
pub const MAIN_MINIMIZE_BUTTON: SpriteRect = (9.0, 0.0, 9.0, 9.0);
pub const MAIN_MINIMIZE_BUTTON_SELECTED: SpriteRect = (9.0, 9.0, 9.0, 9.0);
pub const MAIN_SHADE_BUTTON: SpriteRect = (0.0, 18.0, 9.0, 9.0);
pub const MAIN_SHADE_BUTTON_SELECTED: SpriteRect = (9.0, 18.0, 9.0, 9.0);
pub const MAIN_CLOSE_BUTTON: SpriteRect = (18.0, 0.0, 9.0, 9.0);
pub const MAIN_CLOSE_BUTTON_SELECTED: SpriteRect = (18.0, 9.0, 9.0, 9.0);

// Source slices: CBUTTONS.BMP (136x36)
pub const PREV_BUTTON: SpriteRect = (0.0, 0.0, 23.0, 18.0);
pub const PREV_BUTTON_ACTIVE: SpriteRect = (0.0, 18.0, 23.0, 18.0);
pub const PLAY_BUTTON: SpriteRect = (23.0, 0.0, 23.0, 18.0);
pub const PLAY_BUTTON_ACTIVE: SpriteRect = (23.0, 18.0, 23.0, 18.0);
pub const PAUSE_BUTTON: SpriteRect = (46.0, 0.0, 23.0, 18.0);
pub const PAUSE_BUTTON_ACTIVE: SpriteRect = (46.0, 18.0, 23.0, 18.0);
pub const STOP_BUTTON: SpriteRect = (69.0, 0.0, 23.0, 18.0);
pub const STOP_BUTTON_ACTIVE: SpriteRect = (69.0, 18.0, 23.0, 18.0);
pub const NEXT_BUTTON: SpriteRect = (92.0, 0.0, 22.0, 18.0);
pub const NEXT_BUTTON_ACTIVE: SpriteRect = (92.0, 18.0, 22.0, 18.0);
pub const EJECT_BUTTON: SpriteRect = (114.0, 0.0, 22.0, 16.0);
pub const EJECT_BUTTON_ACTIVE: SpriteRect = (114.0, 16.0, 22.0, 16.0);

// Source slices: POSBAR.BMP (307x10)
pub const POSBAR_BG: SpriteRect = (0.0, 0.0, 248.0, 10.0);
pub const POSBAR_THUMB: SpriteRect = (248.0, 0.0, 29.0, 10.0);
pub const POSBAR_THUMB_ACTIVE: SpriteRect = (278.0, 0.0, 29.0, 10.0);

// Source slices: SHUFREP.BMP (92x85)
pub const REPEAT_OFF: SpriteRect = (0.0, 0.0, 28.0, 15.0);
pub const REPEAT_OFF_ACTIVE: SpriteRect = (0.0, 15.0, 28.0, 15.0);
pub const REPEAT_ON: SpriteRect = (0.0, 30.0, 28.0, 15.0);
pub const REPEAT_ON_ACTIVE: SpriteRect = (0.0, 45.0, 28.0, 15.0);
pub const SHUFFLE_OFF: SpriteRect = (28.0, 0.0, 47.0, 15.0);
pub const SHUFFLE_OFF_ACTIVE: SpriteRect = (28.0, 15.0, 47.0, 15.0);
pub const SHUFFLE_ON: SpriteRect = (28.0, 30.0, 47.0, 15.0);
pub const SHUFFLE_ON_ACTIVE: SpriteRect = (28.0, 45.0, 47.0, 15.0);
pub const EQ_BUTTON_OFF: SpriteRect = (0.0, 61.0, 23.0, 12.0);
pub const EQ_BUTTON_OFF_ACTIVE: SpriteRect = (46.0, 61.0, 23.0, 12.0);
pub const EQ_BUTTON_ON: SpriteRect = (0.0, 73.0, 23.0, 12.0);
pub const EQ_BUTTON_ON_ACTIVE: SpriteRect = (46.0, 73.0, 23.0, 12.0);
pub const PL_BUTTON_OFF: SpriteRect = (23.0, 61.0, 23.0, 12.0);
pub const PL_BUTTON_OFF_ACTIVE: SpriteRect = (69.0, 61.0, 23.0, 12.0);
pub const PL_BUTTON_ON: SpriteRect = (23.0, 73.0, 23.0, 12.0);
pub const PL_BUTTON_ON_ACTIVE: SpriteRect = (69.0, 73.0, 23.0, 12.0);

// Source slices: PLAYPAUS.BMP (42x9)
pub const STATUS_PLAYING: SpriteRect = (0.0, 0.0, 9.0, 9.0);
pub const STATUS_PAUSED: SpriteRect = (9.0, 0.0, 9.0, 9.0);
pub const STATUS_STOPPED: SpriteRect = (18.0, 0.0, 9.0, 9.0);

// Source slices: MONOSTER.BMP
pub const STEREO_ON: SpriteRect = (0.0, 0.0, 29.0, 12.0);
pub const STEREO_OFF: SpriteRect = (0.0, 12.0, 29.0, 12.0);
pub const MONO_ON: SpriteRect = (29.0, 0.0, 27.0, 12.0);
pub const MONO_OFF: SpriteRect = (29.0, 12.0, 27.0, 12.0);

// Source slices: NUMBERS.BMP (digits are 9x13)
pub const DIGIT_WIDTH: f32 = 9.0;
pub const DIGIT_HEIGHT: f32 = 13.0;

/// Returns the source rectangle for a digit `0..=9`.
pub fn digit_rect(digit: u8) -> SpriteRect {
    (digit as f32 * DIGIT_WIDTH, 0.0, DIGIT_WIDTH, DIGIT_HEIGHT)
}

// Source slices: VOLUME.BMP
pub const VOLUME_FRAMES: u32 = 28;
pub const VOLUME_BG_WIDTH: f32 = 68.0;
pub const VOLUME_BG_HEIGHT: f32 = 13.0;
pub const VOLUME_BG_STRIDE: f32 = 15.0;
pub const VOLUME_THUMB: SpriteRect = (15.0, 422.0, 14.0, 11.0);
pub const VOLUME_THUMB_ACTIVE: SpriteRect = (0.0, 422.0, 14.0, 11.0);

// Source slices: BALANCE.BMP
pub const BALANCE_FRAMES: u32 = 28;
pub const BALANCE_BG_WIDTH: f32 = 38.0;
pub const BALANCE_BG_HEIGHT: f32 = 13.0;
pub const BALANCE_BG_STRIDE: f32 = 15.0;
pub const BALANCE_BG_X: f32 = 9.0;
pub const BALANCE_THUMB: SpriteRect = (15.0, 422.0, 14.0, 11.0);
pub const BALANCE_THUMB_ACTIVE: SpriteRect = (0.0, 422.0, 14.0, 11.0);

// Fixed layout coordinates in the 275x116 main window
pub const POS_STATUS: (f32, f32) = (26.0, 28.0);
pub const POS_TIME_DIGITS: [(f32, f32); 4] =
    [(48.0, 26.0), (60.0, 26.0), (78.0, 26.0), (90.0, 26.0)];
pub const POS_MONO: (f32, f32) = (212.0, 41.0);
pub const POS_STEREO: (f32, f32) = (239.0, 41.0);

pub const POS_POSBAR: (f32, f32) = (17.0, 72.0);
pub const POS_CBUTTONS: (f32, f32) = (16.0, 88.0);
pub const POS_EJECT: (f32, f32) = (136.0, 89.0);
pub const POS_VOLUME: (f32, f32) = (107.0, 57.0);
pub const POS_BALANCE: (f32, f32) = (177.0, 57.0);
pub const POS_SHUFFLE: (f32, f32) = (164.0, 89.0);
pub const POS_REPEAT: (f32, f32) = (210.0, 89.0);
pub const POS_EQ_BUTTON: (f32, f32) = (219.0, 58.0);
pub const POS_PL_BUTTON: (f32, f32) = (242.0, 58.0);

pub const POS_OPTIONS_BUTTON: (f32, f32) = (6.0, 3.0);
pub const POS_MINIMIZE_BUTTON: (f32, f32) = (244.0, 3.0);
pub const POS_SHADE_BUTTON: (f32, f32) = (254.0, 3.0);
pub const POS_CLOSE_BUTTON: (f32, f32) = (264.0, 3.0);

// EQ/PL panel windows (275x116 top section)
pub const PANEL_WINDOW: SpriteRect = (0.0, 0.0, 275.0, 116.0);

// Equalizer window geometry and source slices (EQMAIN.BMP)
pub const EQ_WIDTH: f32 = 275.0;
pub const EQ_HEIGHT: f32 = 116.0;
pub const EQ_WINDOW: SpriteRect = (0.0, 0.0, EQ_WIDTH, EQ_HEIGHT);
pub const EQ_DRAG_AREA: SpriteRect = (0.0, 0.0, EQ_WIDTH, 14.0);

pub const EQ_TITLE_BAR: SpriteRect = (0.0, 149.0, EQ_WIDTH, 14.0);
pub const EQ_TITLE_BAR_SELECTED: SpriteRect = (0.0, 134.0, EQ_WIDTH, 14.0);
pub const EQ_CLOSE_BUTTON: SpriteRect = (0.0, 116.0, 9.0, 9.0);
pub const EQ_CLOSE_BUTTON_SELECTED: SpriteRect = (0.0, 125.0, 9.0, 9.0);

pub const EQ_ON_BUTTON_OFF: SpriteRect = (10.0, 119.0, 26.0, 12.0);
pub const EQ_ON_BUTTON_OFF_SELECTED: SpriteRect = (128.0, 119.0, 26.0, 12.0);
pub const EQ_ON_BUTTON_ON: SpriteRect = (69.0, 119.0, 26.0, 12.0);
pub const EQ_ON_BUTTON_ON_SELECTED: SpriteRect = (187.0, 119.0, 26.0, 12.0);

pub const EQ_AUTO_BUTTON_OFF: SpriteRect = (36.0, 119.0, 32.0, 12.0);
pub const EQ_AUTO_BUTTON_OFF_SELECTED: SpriteRect = (154.0, 119.0, 32.0, 12.0);
pub const EQ_AUTO_BUTTON_ON: SpriteRect = (95.0, 119.0, 32.0, 12.0);
pub const EQ_AUTO_BUTTON_ON_SELECTED: SpriteRect = (213.0, 119.0, 32.0, 12.0);

pub const EQ_PRESETS_BUTTON: SpriteRect = (224.0, 164.0, 44.0, 12.0);
pub const EQ_PRESETS_BUTTON_SELECTED: SpriteRect = (224.0, 176.0, 44.0, 12.0);

pub const EQ_SLIDER_BG: SpriteRect = (13.0, 229.0, 14.0, 63.0);
pub const EQ_SLIDER_THUMB: SpriteRect = (0.0, 164.0, 11.0, 11.0);
pub const EQ_SLIDER_THUMB_SELECTED: SpriteRect = (0.0, 176.0, 11.0, 11.0);

pub const EQ_GRAPH_BG: SpriteRect = (0.0, 294.0, 113.0, 19.0);
pub const EQ_PREAMP_LINE: SpriteRect = (0.0, 314.0, 113.0, 1.0);

pub const POS_EQ_CLOSE_BUTTON: (f32, f32) = (264.0, 3.0);
pub const POS_EQ_ON_BUTTON: (f32, f32) = (14.0, 18.0);
pub const POS_EQ_AUTO_BUTTON: (f32, f32) = (40.0, 18.0);
pub const POS_EQ_PRESETS_BUTTON: (f32, f32) = (217.0, 18.0);
pub const POS_EQ_GRAPH_BG: (f32, f32) = (86.0, 17.0);
pub const POS_EQ_PREAMP_LINE: (f32, f32) = (86.0, 26.0);

pub const EQ_SLIDER_BG_Y: f32 = 38.0;
pub const EQ_SLIDER_TRACK_HEIGHT: f32 = 63.0;
pub const EQ_SLIDER_THUMB_Y_OFFSET: f32 = 0.0;
pub const EQ_SLIDER_XS: [f32; 11] = [
    21.0, 78.0, 96.0, 114.0, 132.0, 150.0, 168.0, 186.0, 204.0, 222.0, 240.0,
];
pub const EQ_THUMB_XS: [f32; 11] = [
    22.0, 79.0, 97.0, 115.0, 133.0, 151.0, 169.0, 187.0, 205.0, 223.0, 241.0,
];

// Playlist window geometry and source slices (PLEDIT.BMP)
pub const PLAYLIST_WIDTH: f32 = 275.0;
pub const PLAYLIST_HEIGHT: f32 = 203.0;
pub const PLAYLIST_DRAG_AREA: SpriteRect = (0.0, 0.0, PLAYLIST_WIDTH, 20.0);

pub const PLAYLIST_TOP_LEFT_CORNER: SpriteRect = (0.0, 21.0, 25.0, 20.0);
pub const PLAYLIST_TOP_TILE: SpriteRect = (127.0, 21.0, 25.0, 20.0);
pub const PLAYLIST_TITLE_BAR: SpriteRect = (26.0, 21.0, 100.0, 20.0);
pub const PLAYLIST_TOP_RIGHT_CORNER: SpriteRect = (153.0, 21.0, 25.0, 20.0);
pub const PLAYLIST_LEFT_TILE: SpriteRect = (0.0, 42.0, 12.0, 29.0);
pub const PLAYLIST_RIGHT_TILE: SpriteRect = (31.0, 42.0, 20.0, 29.0);
pub const PLAYLIST_BOTTOM_LEFT_CORNER: SpriteRect = (0.0, 72.0, 125.0, 38.0);
pub const PLAYLIST_BOTTOM_RIGHT_CORNER: SpriteRect = (126.0, 72.0, 150.0, 38.0);
pub const PLAYLIST_VISUALIZER_BG: SpriteRect = (205.0, 0.0, 75.0, 38.0);
pub const PLAYLIST_SCROLL_HANDLE: SpriteRect = (52.0, 53.0, 8.0, 18.0);
pub const PLAYLIST_SCROLL_HANDLE_SELECTED: SpriteRect = (61.0, 53.0, 8.0, 18.0);

pub const PLAYLIST_TOP_TILE_XS: [f32; 6] = [25.0, 50.0, 75.0, 175.0, 200.0, 225.0];
pub const PLAYLIST_SIDE_TILE_YS: [f32; 5] = [20.0, 49.0, 78.0, 107.0, 136.0];
pub const POS_PLAYLIST_TITLE_BAR: (f32, f32) = (87.0, 0.0);
pub const POS_PLAYLIST_TOP_RIGHT: (f32, f32) = (250.0, 0.0);
pub const POS_PLAYLIST_RIGHT_X: f32 = 255.0;
pub const POS_PLAYLIST_BOTTOM_LEFT: (f32, f32) = (0.0, 165.0);
pub const POS_PLAYLIST_BOTTOM_RIGHT: (f32, f32) = (125.0, 165.0);
pub const POS_PLAYLIST_VIS_BG: (f32, f32) = (200.0, 165.0);
pub const PLAYLIST_LIST_BG: SpriteRect = (12.0, 20.0, 243.0, 145.0);
pub const PLAYLIST_SCROLL_TRACK: SpriteRect = (260.0, 20.0, 8.0, 145.0);
