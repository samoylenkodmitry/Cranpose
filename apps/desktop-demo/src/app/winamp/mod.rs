//! Winamp skin renderer demo tab.
//!
//! UI is intentionally split into per-control composables instead of a
//! monolithic draw pass so interactions and sprite mapping stay explicit.

#![allow(non_snake_case)]

mod skin;
mod sprites;

use std::rc::Rc;
use std::sync::OnceLock;

use cranpose::{
    rememberWindowState, Window, WindowConfig, WindowModifierExt, WindowResizeDirection,
    WindowState,
};
use cranpose_core::{self, MutableState};
use cranpose_foundation::PointerButton;
use cranpose_ui::{
    composable, current_density, Box, BoxSpec, Button, Canvas, Color, Column, ColumnSpec, Modifier,
    Point, PointerEventKind, PointerInputScope, Size, Text, TextStyle,
};
use cranpose_ui_graphics::{ImageBitmap, Rect};

use skin::{load_skin, WinampSkin};
use sprites::*;

fn winamp_press_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("WINAMP_PRESS_DEBUG").is_some())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
struct WinampState {
    playback: PlaybackState,
    shuffle: bool,
    repeat: bool,
    eq_visible: bool,
    playlist_visible: bool,
    eq_enabled: bool,
    eq_auto: bool,
    eq_values: [f32; 11],
    playlist_scroll: f32,
    volume: f32,
    balance: f32,
    position: f32,
    status: String,
}

impl Default for WinampState {
    fn default() -> Self {
        Self {
            playback: PlaybackState::Stopped,
            shuffle: false,
            repeat: false,
            eq_visible: true,
            playlist_visible: true,
            eq_enabled: true,
            eq_auto: false,
            eq_values: [0.5; 11],
            playlist_scroll: 0.0,
            volume: 0.72,
            balance: 0.5,
            position: 0.25,
            status: "Stopped".to_string(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum WinampDragTarget {
    Inline(MutableState<Point>),
    NativeGroup {
        windows: WinampNativeWindowStates,
        dragged: WinampWindowId,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum WinampWindowId {
    Main,
    Equalizer,
    Playlist,
}

#[derive(Clone, Copy, PartialEq)]
enum WinampWindowSize {
    Fixed(Size),
    State(WindowState),
}

impl WinampWindowSize {
    fn get(self) -> Size {
        match self {
            Self::Fixed(size) => size,
            Self::State(state) => state.size(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct WinampTabState {
    player: MutableState<WinampState>,
    detached: MutableState<bool>,
    inline_windows: WinampInlineWindowStates,
    native_windows: WinampNativeWindowStates,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct WinampInlineWindowStates {
    main: MutableState<Point>,
    equalizer: MutableState<Point>,
    playlist: MutableState<Point>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct WinampNativeWindowStates {
    main: WindowState,
    equalizer: WindowState,
    playlist: WindowState,
}

#[derive(Clone, Copy)]
struct WinampWindowPlacement {
    title: &'static str,
    host_position: Point,
    state: WindowState,
}

#[composable]
pub(crate) fn remember_winamp_tab_state() -> WinampTabState {
    WinampTabState {
        player: cranpose_core::useState(WinampState::default),
        detached: cranpose_core::useState(native_winamp_windows_available),
        inline_windows: WinampInlineWindowStates {
            main: cranpose_core::useState(|| Point::new(26.0, 22.0)),
            equalizer: cranpose_core::useState(|| Point::new(26.0, 142.0)),
            playlist: cranpose_core::useState(|| Point::new(336.0, 22.0)),
        },
        native_windows: WinampNativeWindowStates {
            main: rememberWindowState(MAIN_WIDTH, MAIN_HEIGHT),
            equalizer: rememberWindowState(EQ_WIDTH, EQ_HEIGHT),
            playlist: rememberWindowState(PLAYLIST_WIDTH, PLAYLIST_HEIGHT),
        },
    }
}

#[composable]
pub(crate) fn WinampTab(tab_state: WinampTabState) {
    let scale = ui_scale();
    let state = tab_state.player;
    let native_available = native_winamp_windows_available();
    let detached = native_available && tab_state.detached.get();
    let snapshot = state.get();
    let skin = match remember_winamp_skin() {
        Ok(skin) => skin,
        Err(error) => {
            WinampSkinError(error);
            return;
        }
    };

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(10.0)
            .background(Color(0.05, 0.06, 0.08, 1.0))
            .rounded_corners(12.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!(
                    "{} | pos {:>3.0}% vol {:>3.0}% bal {:>3.0}%",
                    snapshot.status,
                    snapshot.position * 100.0,
                    snapshot.volume * 100.0,
                    snapshot.balance * 100.0,
                ),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            if native_available {
                DockToggleButton(tab_state.detached, detached);
            }

            if !detached {
                WinampInlineStage(skin.clone(), state, tab_state.inline_windows, scale);
            } else {
                WinampNativeWindows(
                    skin.clone(),
                    state,
                    tab_state.inline_windows,
                    tab_state.native_windows,
                    scale,
                    snapshot.clone(),
                );
            }
        },
    );
}

fn remember_winamp_skin() -> Result<WinampSkin, String> {
    cranpose_core::remember(|| {
        let wsz = include_bytes!("../../../assets/winamp.wsz");
        load_skin(wsz).map_err(|err| format!("{err:#}"))
    })
    .with(|result| result.clone())
}

#[composable]
fn WinampSkinError(error: String) {
    Column(
        Modifier::empty().padding(16.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Failed to load Winamp skin",
                Modifier::empty(),
                TextStyle::default(),
            );
            Text(error.clone(), Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
fn DockToggleButton(detached_state: MutableState<bool>, detached: bool) {
    Button(
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.18, 0.34, 0.58, 1.0))
            .rounded_corners(8.0)
            .padding(8.0),
        move || {
            detached_state.set(!detached_state.get_non_reactive());
        },
        move || {
            Text(
                if detached { "Dock" } else { "Undock" },
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn WinampInlineStage(
    skin: WinampSkin,
    state: MutableState<WinampState>,
    windows: WinampInlineWindowStates,
    scale: f32,
) {
    Box(
        Modifier::empty()
            .fill_max_size()
            .clip_to_bounds()
            .background(Color(0.02, 0.02, 0.03, 1.0))
            .rounded_corners(8.0),
        BoxSpec::default(),
        move || {
            MainWindow(
                skin.clone(),
                state,
                WinampDragTarget::Inline(windows.main),
                scale,
            );

            if state.get().eq_visible {
                EqualizerWindow(
                    skin.clone(),
                    state,
                    WinampDragTarget::Inline(windows.equalizer),
                    scale,
                );
            }

            if state.get().playlist_visible {
                PlaylistWindow(
                    skin.pledit.clone(),
                    state,
                    WinampDragTarget::Inline(windows.playlist),
                    WinampWindowSize::Fixed(Size::new(PLAYLIST_WIDTH, PLAYLIST_HEIGHT)),
                    scale,
                );
            }
        },
    );
}

#[composable]
fn WinampNativeWindows(
    skin: WinampSkin,
    state: MutableState<WinampState>,
    inline_windows: WinampInlineWindowStates,
    native_windows: WinampNativeWindowStates,
    scale: f32,
    snapshot: WinampState,
) {
    Window(
        "winamp-main",
        winamp_window_config(
            WinampWindowPlacement {
                title: "Winamp",
                host_position: inline_windows.main.get(),
                state: native_windows.main,
            },
            native_windows,
            WinampWindowId::Main,
        ),
        {
            let skin = skin.clone();
            move || {
                MainWindow(
                    skin.clone(),
                    state,
                    WinampDragTarget::NativeGroup {
                        windows: native_windows,
                        dragged: WinampWindowId::Main,
                    },
                    scale,
                );
            }
        },
    );

    if snapshot.eq_visible {
        Window(
            "winamp-equalizer",
            winamp_window_config(
                WinampWindowPlacement {
                    title: "Winamp Equalizer",
                    host_position: inline_windows.equalizer.get(),
                    state: native_windows.equalizer,
                },
                native_windows,
                WinampWindowId::Equalizer,
            ),
            {
                let skin = skin.clone();
                move || {
                    EqualizerWindow(
                        skin.clone(),
                        state,
                        WinampDragTarget::NativeGroup {
                            windows: native_windows,
                            dragged: WinampWindowId::Equalizer,
                        },
                        scale,
                    );
                }
            },
        );
    }

    if snapshot.playlist_visible {
        Window(
            "winamp-playlist",
            winamp_window_config(
                WinampWindowPlacement {
                    title: "Winamp Playlist",
                    host_position: inline_windows.playlist.get(),
                    state: native_windows.playlist,
                },
                native_windows,
                WinampWindowId::Playlist,
            )
            .with_resizable(true)
            .with_min_size(
                scaled(PLAYLIST_WIDTH, scale),
                scaled(PLAYLIST_HEIGHT, scale),
            ),
            {
                let pledit = skin.pledit.clone();
                move || {
                    PlaylistWindow(
                        pledit.clone(),
                        state,
                        WinampDragTarget::NativeGroup {
                            windows: native_windows,
                            dragged: WinampWindowId::Playlist,
                        },
                        WinampWindowSize::State(native_windows.playlist),
                        scale,
                    );
                }
            },
        );
    }
}

#[composable]
fn MainWindow(
    skin: WinampSkin,
    state: MutableState<WinampState>,
    drag_target: WinampDragTarget,
    scale: f32,
) {
    let snapshot = state.get();

    Box(
        winamp_window_modifier(MAIN_WIDTH, MAIN_HEIGHT, scale, drag_target),
        BoxSpec::default(),
        move || {
            Sprite(skin.main.clone(), MAIN_WINDOW, 0.0, 0.0, scale);
            Sprite(
                skin.titlebar.clone(),
                MAIN_TITLE_BAR_SELECTED,
                0.0,
                0.0,
                scale,
            );

            WindowDragHandle(drag_target, TITLE_DRAG_AREA, scale);

            {
                let state_click = state;
                PressableSprite(
                    skin.titlebar.clone(),
                    MAIN_OPTIONS_BUTTON,
                    MAIN_OPTIONS_BUTTON_SELECTED,
                    POS_OPTIONS_BUTTON.0,
                    POS_OPTIONS_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| s.status = "Options".to_string());
                    },
                );
            }
            {
                let state_click = state;
                PressableSprite(
                    skin.titlebar.clone(),
                    MAIN_MINIMIZE_BUTTON,
                    MAIN_MINIMIZE_BUTTON_SELECTED,
                    POS_MINIMIZE_BUTTON.0,
                    POS_MINIMIZE_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| s.status = "Minimize".to_string());
                    },
                );
            }
            {
                let state_click = state;
                PressableSprite(
                    skin.titlebar.clone(),
                    MAIN_SHADE_BUTTON,
                    MAIN_SHADE_BUTTON_SELECTED,
                    POS_SHADE_BUTTON.0,
                    POS_SHADE_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| s.status = "Shade".to_string());
                    },
                );
            }
            {
                let state_click = state;
                PressableSprite(
                    skin.titlebar.clone(),
                    MAIN_CLOSE_BUTTON,
                    MAIN_CLOSE_BUTTON_SELECTED,
                    POS_CLOSE_BUTTON.0,
                    POS_CLOSE_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| s.status = "Close".to_string());
                    },
                );
            }

            let status_sprite = match snapshot.playback {
                PlaybackState::Stopped => STATUS_STOPPED,
                PlaybackState::Playing => STATUS_PLAYING,
                PlaybackState::Paused => STATUS_PAUSED,
            };
            Sprite(
                skin.playpaus.clone(),
                status_sprite,
                POS_STATUS.0,
                POS_STATUS.1,
                scale,
            );

            let digits = time_digits(snapshot.position);
            for (i, digit) in digits.iter().enumerate() {
                let pos = POS_TIME_DIGITS[i];
                Sprite(
                    skin.numbers.clone(),
                    digit_rect(*digit),
                    pos.0,
                    pos.1,
                    scale,
                );
            }

            Sprite(
                skin.monoster.clone(),
                MONO_OFF,
                POS_MONO.0,
                POS_MONO.1,
                scale,
            );
            Sprite(
                skin.monoster.clone(),
                STEREO_OFF,
                POS_STEREO.0,
                POS_STEREO.1,
                scale,
            );

            Sprite(
                skin.posbar.clone(),
                POSBAR_BG,
                POS_POSBAR.0,
                POS_POSBAR.1,
                scale,
            );
            let position_thumb_x = slider_thumb_x(snapshot.position, POSBAR_BG.2, POSBAR_THUMB.2);
            Sprite(
                skin.posbar.clone(),
                POSBAR_THUMB,
                POS_POSBAR.0 + position_thumb_x,
                POS_POSBAR.1,
                scale,
            );
            {
                let state_drag = state;
                DragSlider(
                    POS_POSBAR.0,
                    POS_POSBAR.1,
                    POSBAR_BG.2,
                    POSBAR_BG.3,
                    scale,
                    move |fraction| {
                        state_drag.update(|s| s.position = fraction);
                    },
                );
            }

            TransportButtons(skin.cbuttons.clone(), state, scale);

            let vol_frame = slider_frame(snapshot.volume, VOLUME_FRAMES);
            Sprite(
                skin.volume.clone(),
                (
                    0.0,
                    vol_frame as f32 * VOLUME_BG_STRIDE,
                    VOLUME_BG_WIDTH,
                    VOLUME_BG_HEIGHT,
                ),
                POS_VOLUME.0,
                POS_VOLUME.1,
                scale,
            );
            let volume_thumb_x = slider_thumb_x(snapshot.volume, VOLUME_BG_WIDTH, VOLUME_THUMB.2);
            Sprite(
                skin.volume.clone(),
                VOLUME_THUMB,
                POS_VOLUME.0 + volume_thumb_x,
                POS_VOLUME.1 + 1.0,
                scale,
            );
            {
                let state_drag = state;
                DragSlider(
                    POS_VOLUME.0,
                    POS_VOLUME.1,
                    VOLUME_BG_WIDTH,
                    VOLUME_BG_HEIGHT,
                    scale,
                    move |fraction| {
                        state_drag.update(|s| s.volume = fraction);
                    },
                );
            }

            let bal_frame = slider_frame(snapshot.balance, BALANCE_FRAMES);
            Sprite(
                skin.balance.clone(),
                (
                    BALANCE_BG_X,
                    bal_frame as f32 * BALANCE_BG_STRIDE,
                    BALANCE_BG_WIDTH,
                    BALANCE_BG_HEIGHT,
                ),
                POS_BALANCE.0,
                POS_BALANCE.1,
                scale,
            );
            let balance_thumb_x =
                slider_thumb_x(snapshot.balance, BALANCE_BG_WIDTH, BALANCE_THUMB.2);
            Sprite(
                skin.balance.clone(),
                BALANCE_THUMB,
                POS_BALANCE.0 + balance_thumb_x,
                POS_BALANCE.1 + 1.0,
                scale,
            );
            {
                let state_drag = state;
                DragSlider(
                    POS_BALANCE.0,
                    POS_BALANCE.1,
                    BALANCE_BG_WIDTH,
                    BALANCE_BG_HEIGHT,
                    scale,
                    move |fraction| {
                        state_drag.update(|s| s.balance = fraction);
                    },
                );
            }

            let shuffle_normal = if snapshot.shuffle {
                SHUFFLE_ON
            } else {
                SHUFFLE_OFF
            };
            let shuffle_pressed = if snapshot.shuffle {
                SHUFFLE_ON_ACTIVE
            } else {
                SHUFFLE_OFF_ACTIVE
            };
            {
                let state_click = state;
                PressableSprite(
                    skin.shufrep.clone(),
                    shuffle_normal,
                    shuffle_pressed,
                    POS_SHUFFLE.0,
                    POS_SHUFFLE.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.shuffle = !s.shuffle;
                            s.status = if s.shuffle {
                                "Shuffle On".to_string()
                            } else {
                                "Shuffle Off".to_string()
                            };
                        });
                    },
                );
            }

            let repeat_normal = if snapshot.repeat {
                REPEAT_ON
            } else {
                REPEAT_OFF
            };
            let repeat_pressed = if snapshot.repeat {
                REPEAT_ON_ACTIVE
            } else {
                REPEAT_OFF_ACTIVE
            };
            {
                let state_click = state;
                PressableSprite(
                    skin.shufrep.clone(),
                    repeat_normal,
                    repeat_pressed,
                    POS_REPEAT.0,
                    POS_REPEAT.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.repeat = !s.repeat;
                            s.status = if s.repeat {
                                "Repeat On".to_string()
                            } else {
                                "Repeat Off".to_string()
                            };
                        });
                    },
                );
            }

            let eq_normal = if snapshot.eq_visible {
                EQ_BUTTON_ON
            } else {
                EQ_BUTTON_OFF
            };
            let eq_pressed = if snapshot.eq_visible {
                EQ_BUTTON_ON_ACTIVE
            } else {
                EQ_BUTTON_OFF_ACTIVE
            };
            {
                let state_click = state;
                PressableSprite(
                    skin.shufrep.clone(),
                    eq_normal,
                    eq_pressed,
                    POS_EQ_BUTTON.0,
                    POS_EQ_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.eq_visible = !s.eq_visible;
                            s.status = if s.eq_visible {
                                "Equalizer Shown".to_string()
                            } else {
                                "Equalizer Hidden".to_string()
                            };
                        });
                    },
                );
            }

            let pl_normal = if snapshot.playlist_visible {
                PL_BUTTON_ON
            } else {
                PL_BUTTON_OFF
            };
            let pl_pressed = if snapshot.playlist_visible {
                PL_BUTTON_ON_ACTIVE
            } else {
                PL_BUTTON_OFF_ACTIVE
            };
            {
                let state_click = state;
                PressableSprite(
                    skin.shufrep.clone(),
                    pl_normal,
                    pl_pressed,
                    POS_PL_BUTTON.0,
                    POS_PL_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.playlist_visible = !s.playlist_visible;
                            s.status = if s.playlist_visible {
                                "Playlist Shown".to_string()
                            } else {
                                "Playlist Hidden".to_string()
                            };
                        });
                    },
                );
            }
        },
    );
}

#[composable]
fn EqualizerWindow(
    skin: WinampSkin,
    state: MutableState<WinampState>,
    drag_target: WinampDragTarget,
    scale: f32,
) {
    let snapshot = state.get();

    Box(
        winamp_window_modifier(EQ_WIDTH, EQ_HEIGHT, scale, drag_target),
        BoxSpec::default(),
        move || {
            Sprite(skin.eqmain.clone(), EQ_WINDOW, 0.0, 0.0, scale);
            Sprite(skin.eqmain.clone(), EQ_TITLE_BAR_SELECTED, 0.0, 0.0, scale);
            Sprite(
                skin.eqmain.clone(),
                EQ_GRAPH_BG,
                POS_EQ_GRAPH_BG.0,
                POS_EQ_GRAPH_BG.1,
                scale,
            );
            Sprite(
                skin.eqmain.clone(),
                EQ_PREAMP_LINE,
                POS_EQ_PREAMP_LINE.0,
                POS_EQ_PREAMP_LINE.1,
                scale,
            );

            WindowDragHandle(drag_target, EQ_DRAG_AREA, scale);

            {
                let state_click = state;
                PressableSprite(
                    skin.eqmain.clone(),
                    EQ_CLOSE_BUTTON,
                    EQ_CLOSE_BUTTON_SELECTED,
                    POS_EQ_CLOSE_BUTTON.0,
                    POS_EQ_CLOSE_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.eq_visible = false;
                            s.status = "Equalizer Hidden".to_string();
                        });
                    },
                );
            }

            let eq_on_normal = if snapshot.eq_enabled {
                EQ_ON_BUTTON_ON
            } else {
                EQ_ON_BUTTON_OFF
            };
            let eq_on_pressed = if snapshot.eq_enabled {
                EQ_ON_BUTTON_ON_SELECTED
            } else {
                EQ_ON_BUTTON_OFF_SELECTED
            };
            {
                let state_click = state;
                PressableSprite(
                    skin.eqmain.clone(),
                    eq_on_normal,
                    eq_on_pressed,
                    POS_EQ_ON_BUTTON.0,
                    POS_EQ_ON_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.eq_enabled = !s.eq_enabled;
                            s.status = if s.eq_enabled {
                                "EQ On".to_string()
                            } else {
                                "EQ Off".to_string()
                            };
                        });
                    },
                );
            }

            let eq_auto_normal = if snapshot.eq_auto {
                EQ_AUTO_BUTTON_ON
            } else {
                EQ_AUTO_BUTTON_OFF
            };
            let eq_auto_pressed = if snapshot.eq_auto {
                EQ_AUTO_BUTTON_ON_SELECTED
            } else {
                EQ_AUTO_BUTTON_OFF_SELECTED
            };
            {
                let state_click = state;
                PressableSprite(
                    skin.eqmain.clone(),
                    eq_auto_normal,
                    eq_auto_pressed,
                    POS_EQ_AUTO_BUTTON.0,
                    POS_EQ_AUTO_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.eq_auto = !s.eq_auto;
                            s.status = if s.eq_auto {
                                "EQ Auto On".to_string()
                            } else {
                                "EQ Auto Off".to_string()
                            };
                        });
                    },
                );
            }

            {
                let state_click = state;
                PressableSprite(
                    skin.eqmain.clone(),
                    EQ_PRESETS_BUTTON,
                    EQ_PRESETS_BUTTON_SELECTED,
                    POS_EQ_PRESETS_BUTTON.0,
                    POS_EQ_PRESETS_BUTTON.1,
                    scale,
                    move || {
                        state_click.update(|s| {
                            s.eq_values = [0.5; 11];
                            s.status = "EQ Reset".to_string();
                        });
                    },
                );
            }

            for (index, slider_x) in EQ_SLIDER_XS.iter().copied().enumerate() {
                let thumb_x = EQ_THUMB_XS[index];
                let value = snapshot.eq_values[index];
                let thumb_y = EQ_SLIDER_BG_Y
                    + vertical_slider_thumb_y(value, EQ_SLIDER_TRACK_HEIGHT, EQ_SLIDER_THUMB.3);

                Sprite(
                    skin.eqmain.clone(),
                    EQ_SLIDER_BG,
                    slider_x,
                    EQ_SLIDER_BG_Y,
                    scale,
                );
                Sprite(
                    skin.eqmain.clone(),
                    EQ_SLIDER_THUMB,
                    thumb_x,
                    thumb_y + EQ_SLIDER_THUMB_Y_OFFSET,
                    scale,
                );

                let state_drag = state;
                VerticalDragSlider(
                    slider_x,
                    EQ_SLIDER_BG_Y,
                    EQ_SLIDER_BG.2,
                    EQ_SLIDER_TRACK_HEIGHT,
                    scale,
                    true,
                    move |fraction| {
                        state_drag.update(|s| {
                            s.eq_values[index] = fraction;
                        });
                    },
                );
            }
        },
    );
}

#[composable]
fn PlaylistWindow(
    pledit: ImageBitmap,
    state: MutableState<WinampState>,
    drag_target: WinampDragTarget,
    window_size: WinampWindowSize,
    scale: f32,
) {
    let snapshot = state.get();
    let window_size = window_size.get();
    let skin_scale = scale.max(f32::EPSILON);
    let width = (window_size.width / skin_scale).max(PLAYLIST_WIDTH);
    let height = (window_size.height / skin_scale).max(PLAYLIST_HEIGHT);
    let right_x = width - PLAYLIST_RIGHT_TILE.2;
    let bottom_y = height - PLAYLIST_BOTTOM_LEFT_CORNER.3;
    let list_width = (right_x - PLAYLIST_LIST_BG.0).max(1.0);
    let list_height = (bottom_y - PLAYLIST_LIST_BG.1).max(1.0);
    let title_min_x = PLAYLIST_TOP_LEFT_CORNER.2;
    let title_max_x = (width - PLAYLIST_TOP_RIGHT_CORNER.2 - PLAYLIST_TITLE_BAR.2).max(title_min_x);
    let title_x = ((width - PLAYLIST_TITLE_BAR.2) * 0.5).clamp(title_min_x, title_max_x);
    let scroll_track_x = width - 15.0;

    Box(
        winamp_window_modifier(width, height, scale, drag_target),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size_points(scaled(list_width, scale), scaled(list_height, scale))
                    .absolute_offset(
                        scaled(PLAYLIST_LIST_BG.0, scale),
                        scaled(PLAYLIST_LIST_BG.1, scale),
                    )
                    .background(Color(0.0, 0.0, 0.0, 1.0)),
                BoxSpec::default(),
                || {},
            );

            Sprite(pledit.clone(), PLAYLIST_TOP_LEFT_CORNER, 0.0, 0.0, scale);
            StretchSprite(
                pledit.clone(),
                PLAYLIST_TOP_TILE,
                PLAYLIST_TOP_LEFT_CORNER.2,
                0.0,
                width - PLAYLIST_TOP_LEFT_CORNER.2 - PLAYLIST_TOP_RIGHT_CORNER.2,
                PLAYLIST_TOP_TILE.3,
                scale,
            );
            Sprite(pledit.clone(), PLAYLIST_TITLE_BAR, title_x, 0.0, scale);
            Sprite(
                pledit.clone(),
                PLAYLIST_TOP_RIGHT_CORNER,
                width - PLAYLIST_TOP_RIGHT_CORNER.2,
                0.0,
                scale,
            );

            StretchSprite(
                pledit.clone(),
                PLAYLIST_LEFT_TILE,
                0.0,
                PLAYLIST_TOP_LEFT_CORNER.3,
                PLAYLIST_LEFT_TILE.2,
                bottom_y - PLAYLIST_TOP_LEFT_CORNER.3,
                scale,
            );
            StretchSprite(
                pledit.clone(),
                PLAYLIST_RIGHT_TILE,
                right_x,
                PLAYLIST_TOP_RIGHT_CORNER.3,
                PLAYLIST_RIGHT_TILE.2,
                bottom_y - PLAYLIST_TOP_RIGHT_CORNER.3,
                scale,
            );

            StretchSprite(
                pledit.clone(),
                PLAYLIST_BOTTOM_LEFT_CORNER,
                0.0,
                bottom_y,
                width - PLAYLIST_BOTTOM_RIGHT_CORNER.2,
                PLAYLIST_BOTTOM_LEFT_CORNER.3,
                scale,
            );
            Sprite(
                pledit.clone(),
                PLAYLIST_BOTTOM_RIGHT_CORNER,
                width - PLAYLIST_BOTTOM_RIGHT_CORNER.2,
                bottom_y,
                scale,
            );
            let scroll_y = PLAYLIST_LIST_BG.1
                + vertical_slider_thumb_y_down(
                    snapshot.playlist_scroll,
                    list_height,
                    PLAYLIST_SCROLL_HANDLE.3,
                );
            Sprite(
                pledit.clone(),
                PLAYLIST_SCROLL_HANDLE,
                scroll_track_x,
                scroll_y,
                scale,
            );

            {
                let state_drag = state;
                VerticalDragSlider(
                    scroll_track_x,
                    PLAYLIST_LIST_BG.1,
                    PLAYLIST_SCROLL_TRACK.2,
                    list_height,
                    scale,
                    false,
                    move |fraction| {
                        state_drag.update(|s| s.playlist_scroll = fraction);
                    },
                );
            }

            WindowDragHandle(drag_target, (0.0, 0.0, width, PLAYLIST_DRAG_AREA.3), scale);
            WindowResizeHandle(
                drag_target,
                WindowResizeDirection::SouthEast,
                width - 16.0,
                height - 16.0,
                16.0,
                16.0,
                scale,
            );
        },
    );
}

#[composable]
fn Sprite(image: ImageBitmap, source: SpriteRect, x: f32, y: f32, scale: f32) {
    let w = scaled(source.2, scale);
    let h = scaled(source.3, scale);
    Canvas(
        Modifier::empty()
            .size_points(w, h)
            .absolute_offset(scaled(x, scale), scaled(y, scale)),
        move |scope| {
            let dst = Rect {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
            };
            scope.draw_image_src(image.clone(), to_rect(source), dst, 1.0, None);
        },
    );
}

#[composable]
fn StretchSprite(
    image: ImageBitmap,
    source: SpriteRect,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
) {
    let w = scaled(width.max(1.0), scale);
    let h = scaled(height.max(1.0), scale);
    Canvas(
        Modifier::empty()
            .size_points(w, h)
            .absolute_offset(scaled(x, scale), scaled(y, scale)),
        move |scope| {
            let dst = Rect {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
            };
            scope.draw_image_src(image.clone(), to_rect(source), dst, 1.0, None);
        },
    );
}

#[composable]
fn PressableSprite(
    image: ImageBitmap,
    normal: SpriteRect,
    pressed: SpriteRect,
    x: f32,
    y: f32,
    scale: f32,
    on_click: impl Fn() + 'static,
) {
    let is_pressed = cranpose_core::useState(|| false);
    let on_click = Rc::new(on_click);

    let current = if is_pressed.get() { pressed } else { normal };
    if winamp_press_debug_enabled() {
        eprintln!(
            "[WINAMP_PRESS_DEBUG] compose button at ({:.1},{:.1}) pressed={} sprite=({:.1},{:.1},{:.1},{:.1})",
            x,
            y,
            is_pressed.get(),
            current.0,
            current.1,
            current.2,
            current.3
        );
    }
    let w = scaled(normal.2, scale);
    let h = scaled(normal.3, scale);

    Canvas(
        Modifier::empty()
            .size_points(w, h)
            .absolute_offset(scaled(x, scale), scaled(y, scale))
            .pointer_input((), {
                move |scope: PointerInputScope| {
                    let on_click = on_click.clone();
                    async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down => {
                                            if winamp_press_debug_enabled() {
                                                eprintln!(
                                                    "[WINAMP_PRESS_DEBUG] down button ({:.1},{:.1}) local=({:.2},{:.2})",
                                                    x, y, event.position.x, event.position.y
                                                );
                                            }
                                            is_pressed.set(true);
                                            event.consume();
                                        }
                                        PointerEventKind::Move => {
                                            if is_pressed.get()
                                                && !event.buttons.contains(PointerButton::Primary)
                                            {
                                                if winamp_press_debug_enabled() {
                                                    eprintln!(
                                                        "[WINAMP_PRESS_DEBUG] move-clears button ({:.1},{:.1})",
                                                        x, y
                                                    );
                                                }
                                                is_pressed.set(false);
                                            }
                                        }
                                        PointerEventKind::Up => {
                                            let was_pressed = is_pressed.get();
                                            is_pressed.set(false);
                                            let inside = event.position.x >= 0.0
                                                && event.position.x <= w
                                                && event.position.y >= 0.0
                                                && event.position.y <= h;
                                            if winamp_press_debug_enabled() {
                                                eprintln!(
                                                    "[WINAMP_PRESS_DEBUG] up button ({:.1},{:.1}) was_pressed={} inside={} local=({:.2},{:.2})",
                                                    x, y, was_pressed, inside, event.position.x, event.position.y
                                                );
                                            }
                                            if was_pressed && inside {
                                                if winamp_press_debug_enabled() {
                                                    eprintln!(
                                                        "[WINAMP_PRESS_DEBUG] click fired button ({:.1},{:.1})",
                                                        x, y
                                                    );
                                                }
                                                on_click();
                                            }
                                            event.consume();
                                        }
                                        PointerEventKind::Cancel => {
                                            if winamp_press_debug_enabled() {
                                                eprintln!(
                                                    "[WINAMP_PRESS_DEBUG] cancel button ({:.1},{:.1})",
                                                    x, y
                                                );
                                            }
                                            is_pressed.set(false);
                                        }
                                        PointerEventKind::Scroll
                                        | PointerEventKind::Enter
                                        | PointerEventKind::Exit => {}
                                    }
                                }
                            })
                            .await;
                    }
                }
            }),
        move |scope| {
            let dst = Rect {
                x: 0.0,
                y: 0.0,
                width: scaled(current.2, scale),
                height: scaled(current.3, scale),
            };
            scope.draw_image_src(image.clone(), to_rect(current), dst, 1.0, None);
        },
    );
}

#[composable]
fn DragSlider(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
    on_change: impl Fn(f32) + 'static,
) {
    let on_change = Rc::new(on_change);

    Box(
        Modifier::empty()
            .size_points(scaled(width, scale), scaled(height, scale))
            .absolute_offset(scaled(x, scale), scaled(y, scale))
            .pointer_input((), {
                move |scope: PointerInputScope| {
                    let on_change = on_change.clone();
                    async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                let mut dragging = false;
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down => {
                                            dragging = true;
                                            let value = (event.position.x / scaled(width, scale))
                                                .clamp(0.0, 1.0);
                                            on_change(value);
                                            event.consume();
                                        }
                                        PointerEventKind::Move if dragging => {
                                            let value = (event.position.x / scaled(width, scale))
                                                .clamp(0.0, 1.0);
                                            on_change(value);
                                            event.consume();
                                        }
                                        PointerEventKind::Up | PointerEventKind::Cancel => {
                                            dragging = false;
                                        }
                                        _ => {}
                                    }
                                }
                            })
                            .await;
                    }
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn VerticalDragSlider(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
    invert: bool,
    on_change: impl Fn(f32) + 'static,
) {
    let on_change = Rc::new(on_change);

    Box(
        Modifier::empty()
            .size_points(scaled(width, scale), scaled(height, scale))
            .absolute_offset(scaled(x, scale), scaled(y, scale))
            .pointer_input((), {
                move |scope: PointerInputScope| {
                    let on_change = on_change.clone();
                    async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                let mut dragging = false;
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down => {
                                            dragging = true;
                                            let raw = (event.position.y / scaled(height, scale))
                                                .clamp(0.0, 1.0);
                                            on_change(if invert { 1.0 - raw } else { raw });
                                            event.consume();
                                        }
                                        PointerEventKind::Move if dragging => {
                                            let raw = (event.position.y / scaled(height, scale))
                                                .clamp(0.0, 1.0);
                                            on_change(if invert { 1.0 - raw } else { raw });
                                            event.consume();
                                        }
                                        PointerEventKind::Up | PointerEventKind::Cancel => {
                                            dragging = false;
                                        }
                                        _ => {}
                                    }
                                }
                            })
                            .await;
                    }
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn WindowDragHandle(drag_target: WinampDragTarget, area: SpriteRect, scale: f32) {
    let modifier = Modifier::empty()
        .size_points(scaled(area.2, scale), scaled(area.3, scale))
        .absolute_offset(scaled(area.0, scale), scaled(area.1, scale));

    match drag_target {
        WinampDragTarget::NativeGroup { .. } => {
            Box(modifier.window_drag_area(), BoxSpec::default(), || {});
        }
        WinampDragTarget::Inline(window_position) => {
            let drag_offset = cranpose_core::useState(|| None::<Point>);

            Box(
                modifier.pointer_input((), {
                    move |scope: PointerInputScope| async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down => {
                                            let current = window_position.get();
                                            drag_offset.set(Some(Point::new(
                                                event.global_position.x - current.x,
                                                event.global_position.y - current.y,
                                            )));
                                            event.consume();
                                        }
                                        PointerEventKind::Move => {
                                            if !event.buttons.contains(PointerButton::Primary) {
                                                drag_offset.set(None);
                                                continue;
                                            }
                                            if let Some(offset) = drag_offset.get() {
                                                window_position.set(Point::new(
                                                    snap_to_pixel(
                                                        event.global_position.x - offset.x,
                                                    ),
                                                    snap_to_pixel(
                                                        event.global_position.y - offset.y,
                                                    ),
                                                ));
                                                event.consume();
                                            }
                                        }
                                        PointerEventKind::Up | PointerEventKind::Cancel => {
                                            drag_offset.set(None);
                                        }
                                        PointerEventKind::Scroll
                                        | PointerEventKind::Enter
                                        | PointerEventKind::Exit => {}
                                    }
                                }
                            })
                            .await;
                    }
                }),
                BoxSpec::default(),
                || {},
            );
        }
    }
}

#[composable]
fn WindowResizeHandle(
    drag_target: WinampDragTarget,
    direction: WindowResizeDirection,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
) {
    if !matches!(drag_target, WinampDragTarget::NativeGroup { .. }) {
        return;
    }

    Box(
        Modifier::empty()
            .size_points(scaled(width, scale), scaled(height, scale))
            .absolute_offset(scaled(x, scale), scaled(y, scale))
            .window_resize_area(direction),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn TransportButtons(cbuttons: ImageBitmap, state: MutableState<WinampState>, scale: f32) {
    {
        let state_click = state;
        PressableSprite(
            cbuttons.clone(),
            PREV_BUTTON,
            PREV_BUTTON_ACTIVE,
            POS_CBUTTONS.0,
            POS_CBUTTONS.1,
            scale,
            move || {
                state_click.update(|s| s.status = "Previous".to_string());
            },
        );
    }

    {
        let state_click = state;
        PressableSprite(
            cbuttons.clone(),
            PLAY_BUTTON,
            PLAY_BUTTON_ACTIVE,
            POS_CBUTTONS.0 + 23.0,
            POS_CBUTTONS.1,
            scale,
            move || {
                state_click.update(|s| {
                    s.playback = PlaybackState::Playing;
                    s.status = "Play".to_string();
                });
            },
        );
    }

    {
        let state_click = state;
        PressableSprite(
            cbuttons.clone(),
            PAUSE_BUTTON,
            PAUSE_BUTTON_ACTIVE,
            POS_CBUTTONS.0 + 46.0,
            POS_CBUTTONS.1,
            scale,
            move || {
                state_click.update(|s| {
                    s.playback = PlaybackState::Paused;
                    s.status = "Pause".to_string();
                });
            },
        );
    }

    {
        let state_click = state;
        PressableSprite(
            cbuttons.clone(),
            STOP_BUTTON,
            STOP_BUTTON_ACTIVE,
            POS_CBUTTONS.0 + 69.0,
            POS_CBUTTONS.1,
            scale,
            move || {
                state_click.update(|s| {
                    s.playback = PlaybackState::Stopped;
                    s.status = "Stop".to_string();
                });
            },
        );
    }

    {
        let state_click = state;
        PressableSprite(
            cbuttons.clone(),
            NEXT_BUTTON,
            NEXT_BUTTON_ACTIVE,
            POS_CBUTTONS.0 + 92.0,
            POS_CBUTTONS.1,
            scale,
            move || {
                state_click.update(|s| s.status = "Next".to_string());
            },
        );
    }

    {
        let state_click = state;
        PressableSprite(
            cbuttons,
            EJECT_BUTTON,
            EJECT_BUTTON_ACTIVE,
            POS_EJECT.0,
            POS_EJECT.1,
            scale,
            move || {
                state_click.update(|s| s.status = "Open".to_string());
            },
        );
    }
}

const WINAMP_NATIVE_HOST_OFFSET_X: f32 = 640.0;
const WINAMP_NATIVE_HOST_OFFSET_Y: f32 = 118.0;
const WINAMP_ATTACH_EPSILON: f32 = 3.0;
const WINAMP_SNAP_DISTANCE: f32 = 8.0;

fn native_winamp_windows_available() -> bool {
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    {
        std::env::var_os("CRANPOSE_WINAMP_INLINE").is_none()
    }

    #[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
    {
        false
    }
}

fn base_winamp_window_config(placement: WinampWindowPlacement) -> WindowConfig {
    let state_size = placement.state.size();
    WindowConfig::borderless(placement.title, state_size.width, state_size.height)
        .with_host_window_position(
            snap_to_pixel(placement.host_position.x + WINAMP_NATIVE_HOST_OFFSET_X),
            snap_to_pixel(placement.host_position.y + WINAMP_NATIVE_HOST_OFFSET_Y),
        )
        .with_transparent(false)
        .with_resizable(false)
        .with_visible(true)
}

fn winamp_window_config(
    placement: WinampWindowPlacement,
    native_windows: WinampNativeWindowStates,
    window_id: WinampWindowId,
) -> WindowConfig {
    let state = placement.state;
    base_winamp_window_config(placement)
        .on_moved(move |x, y| {
            move_attached_winamp_window(native_windows, window_id, Point::new(x, y));
        })
        .with_state(state)
}

fn move_attached_winamp_window(
    native_windows: WinampNativeWindowStates,
    moved: WinampWindowId,
    new_position: Point,
) {
    let moved_state = native_windows.state(moved);
    let Some(old_position) = moved_state.position_non_reactive() else {
        moved_state.set_position(Some(new_position));
        return;
    };
    if moved != WinampWindowId::Main {
        moved_state.set_position(Some(Point::new(
            snap_to_pixel(new_position.x),
            snap_to_pixel(new_position.y),
        )));
        return;
    }

    let delta = Point::new(
        new_position.x - old_position.x,
        new_position.y - old_position.y,
    );
    move_winamp_component_by_delta(native_windows, WinampWindowId::Main, delta);
}

fn move_winamp_component_by_delta(
    native_windows: WinampNativeWindowStates,
    dragged: WinampWindowId,
    delta: Point,
) {
    if delta.x.abs() <= f32::EPSILON && delta.y.abs() <= f32::EPSILON {
        return;
    }

    let snapshots = winamp_window_snapshots(native_windows);
    let moved = move_winamp_snapshots(&snapshots, dragged, delta);
    apply_winamp_window_snapshots(native_windows, moved);
}

fn apply_winamp_window_snapshots(
    native_windows: WinampNativeWindowStates,
    snapshots: Vec<WinampWindowSnapshot>,
) {
    for snapshot in snapshots {
        native_windows
            .state(snapshot.id)
            .set_position(Some(Point::new(
                snap_to_pixel(snapshot.position.x),
                snap_to_pixel(snapshot.position.y),
            )));
    }
}

impl WinampNativeWindowStates {
    fn state(self, id: WinampWindowId) -> WindowState {
        match id {
            WinampWindowId::Main => self.main,
            WinampWindowId::Equalizer => self.equalizer,
            WinampWindowId::Playlist => self.playlist,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WinampWindowSnapshot {
    id: WinampWindowId,
    position: Point,
    size: Size,
}

fn winamp_window_snapshots(native_windows: WinampNativeWindowStates) -> Vec<WinampWindowSnapshot> {
    [
        WinampWindowId::Main,
        WinampWindowId::Equalizer,
        WinampWindowId::Playlist,
    ]
    .into_iter()
    .filter_map(|id| {
        let state = native_windows.state(id);
        Some(WinampWindowSnapshot {
            id,
            position: state.position_non_reactive()?,
            size: state.size_non_reactive(),
        })
    })
    .collect()
}

fn attached_winamp_component(
    snapshots: &[WinampWindowSnapshot],
    dragged: WinampWindowId,
) -> Vec<WinampWindowId> {
    let mut component = vec![dragged];
    let mut changed = true;

    while changed {
        changed = false;
        for candidate in snapshots {
            if component.contains(&candidate.id) {
                continue;
            }

            let attached_to_component = snapshots
                .iter()
                .filter(|snapshot| component.contains(&snapshot.id))
                .any(|snapshot| {
                    rects_attached(
                        candidate.position,
                        candidate.size,
                        snapshot.position,
                        snapshot.size,
                    )
                });
            if attached_to_component {
                component.push(candidate.id);
                changed = true;
            }
        }
    }

    component
}

fn move_winamp_snapshots(
    snapshots: &[WinampWindowSnapshot],
    dragged: WinampWindowId,
    delta: Point,
) -> Vec<WinampWindowSnapshot> {
    let mut component = attached_winamp_component(snapshots, dragged);
    move_winamp_snapshots_for_component(snapshots, &mut component, delta, true)
}

fn move_winamp_snapshots_for_component(
    snapshots: &[WinampWindowSnapshot],
    component: &mut Vec<WinampWindowId>,
    delta: Point,
    expand_component: bool,
) -> Vec<WinampWindowSnapshot> {
    let mut moved = snapshots.to_vec();
    translate_winamp_snapshots(&mut moved, component, delta);

    if expand_component {
        while let Some(snap) = closest_winamp_snap(&moved, component) {
            translate_winamp_snapshots(&mut moved, component, snap.delta);
            for id in attached_winamp_component(&moved, snap.target) {
                if !component.contains(&id) {
                    component.push(id);
                }
            }
        }
    } else if let Some(snap) = closest_winamp_snap(&moved, component) {
        translate_winamp_snapshots(&mut moved, component, snap.delta);
    }

    moved
}

fn translate_winamp_snapshots(
    snapshots: &mut [WinampWindowSnapshot],
    component: &[WinampWindowId],
    delta: Point,
) {
    if delta.x.abs() <= f32::EPSILON && delta.y.abs() <= f32::EPSILON {
        return;
    }

    for snapshot in snapshots {
        if component.contains(&snapshot.id) {
            snapshot.position.x += delta.x;
            snapshot.position.y += delta.y;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WinampSnap {
    target: WinampWindowId,
    delta: Point,
    distance: f32,
    contact: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WinampSnapCandidate {
    delta: Point,
    contact: f32,
}

fn closest_winamp_snap(
    snapshots: &[WinampWindowSnapshot],
    component: &[WinampWindowId],
) -> Option<WinampSnap> {
    let mut closest = None::<WinampSnap>;

    for moving in snapshots
        .iter()
        .filter(|snapshot| component.contains(&snapshot.id))
    {
        for stationary in snapshots
            .iter()
            .filter(|snapshot| !component.contains(&snapshot.id))
        {
            for candidate in winamp_snap_candidates(*moving, *stationary) {
                let snap = WinampSnap {
                    target: stationary.id,
                    delta: candidate.delta,
                    distance: candidate.delta.x.abs() + candidate.delta.y.abs(),
                    contact: candidate.contact,
                };
                if closest.is_none_or(|current| {
                    snap.contact > current.contact
                        || snap.contact == current.contact && snap.distance < current.distance
                }) {
                    closest = Some(snap);
                }
            }
        }
    }

    closest
}

fn winamp_snap_candidates(
    moving: WinampWindowSnapshot,
    stationary: WinampWindowSnapshot,
) -> Vec<WinampSnapCandidate> {
    let moving_left = moving.position.x;
    let moving_top = moving.position.y;
    let moving_right = moving.position.x + moving.size.width;
    let moving_bottom = moving.position.y + moving.size.height;
    let stationary_left = stationary.position.x;
    let stationary_top = stationary.position.y;
    let stationary_right = stationary.position.x + stationary.size.width;
    let stationary_bottom = stationary.position.y + stationary.size.height;

    let mut candidates = Vec::new();
    if ranges_overlap_strict(moving_top, moving_bottom, stationary_top, stationary_bottom) {
        let contact =
            range_overlap_length(moving_top, moving_bottom, stationary_top, stationary_bottom);
        if near_snap(moving_right, stationary_left) {
            candidates.push(WinampSnapCandidate {
                delta: Point::new(stationary_left - moving_right, 0.0),
                contact,
            });
        }
        if near_snap(moving_left, stationary_right) {
            candidates.push(WinampSnapCandidate {
                delta: Point::new(stationary_right - moving_left, 0.0),
                contact,
            });
        }
    }
    if ranges_overlap_strict(moving_left, moving_right, stationary_left, stationary_right) {
        let contact =
            range_overlap_length(moving_left, moving_right, stationary_left, stationary_right);
        if near_snap(moving_bottom, stationary_top) {
            candidates.push(WinampSnapCandidate {
                delta: Point::new(0.0, stationary_top - moving_bottom),
                contact,
            });
        }
        if near_snap(moving_top, stationary_bottom) {
            candidates.push(WinampSnapCandidate {
                delta: Point::new(0.0, stationary_bottom - moving_top),
                contact,
            });
        }
    }

    candidates
}

fn rects_attached(child: Point, child_size: Size, main: Point, main_size: Size) -> bool {
    let child_right = child.x + child_size.width;
    let child_bottom = child.y + child_size.height;
    let main_right = main.x + main_size.width;
    let main_bottom = main.y + main_size.height;

    let touches_horizontal = near(child.x, main_right) || near(child_right, main.x);
    let overlaps_vertical = ranges_overlap(child.y, child_bottom, main.y, main_bottom);
    let touches_vertical = near(child.y, main_bottom) || near(child_bottom, main.y);
    let overlaps_horizontal = ranges_overlap(child.x, child_right, main.x, main_right);

    touches_horizontal && overlaps_vertical || touches_vertical && overlaps_horizontal
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= WINAMP_ATTACH_EPSILON
}

fn near_snap(a: f32, b: f32) -> bool {
    (a - b).abs() <= WINAMP_SNAP_DISTANCE
}

fn ranges_overlap(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> bool {
    a_start <= b_end + WINAMP_ATTACH_EPSILON && b_start <= a_end + WINAMP_ATTACH_EPSILON
}

fn ranges_overlap_strict(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> bool {
    a_start < b_end && b_start < a_end
}

fn range_overlap_length(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> f32 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

fn winamp_window_modifier(
    width: f32,
    height: f32,
    scale: f32,
    drag_target: WinampDragTarget,
) -> Modifier {
    let modifier = Modifier::empty().size_points(scaled(width, scale), scaled(height, scale));
    match drag_target {
        WinampDragTarget::Inline(position) => {
            let position = position.get();
            modifier.offset(snap_to_pixel(position.x), snap_to_pixel(position.y))
        }
        WinampDragTarget::NativeGroup { .. } => modifier,
    }
}

fn ui_scale() -> f32 {
    // Skin pixel coordinates map directly to dp.  On high-density screens the
    // renderer upscales automatically, keeping the skin at the same visual
    // size as on a 1× desktop display.
    1.0
}

fn snap_to_pixel(value: f32) -> f32 {
    let density = current_density();
    if density > 0.0 {
        (value * density).round() / density
    } else {
        value.round()
    }
}

fn scaled(value: f32, scale: f32) -> f32 {
    snap_to_pixel(value * scale)
}

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn slider_thumb_x(value: f32, bar_width: f32, knob_width: f32) -> f32 {
    clamp01(value) * (bar_width - knob_width)
}

fn slider_frame(value: f32, frames: u32) -> u32 {
    if frames <= 1 {
        return 0;
    }
    let max_index = frames - 1;
    (clamp01(value) * max_index as f32).round() as u32
}

fn vertical_slider_thumb_y(value: f32, track_height: f32, knob_height: f32) -> f32 {
    (1.0 - clamp01(value)) * (track_height - knob_height)
}

fn vertical_slider_thumb_y_down(value: f32, track_height: f32, knob_height: f32) -> f32 {
    clamp01(value) * (track_height - knob_height)
}

fn time_digits(position: f32) -> [u8; 4] {
    let seconds = (clamp01(position) * 300.0).round() as u32;
    let minutes = seconds / 60;
    let remainder = seconds % 60;
    [
        ((minutes / 10) % 10) as u8,
        (minutes % 10) as u8,
        (remainder / 10) as u8,
        (remainder % 10) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_digits_are_mapped_correctly() {
        assert_eq!(time_digits(0.0), [0, 0, 0, 0]);
        assert_eq!(time_digits(1.0), [0, 5, 0, 0]);
    }

    #[test]
    fn slider_helpers_clamp_values() {
        assert_eq!(slider_frame(-1.0, 28), 0);
        assert_eq!(slider_frame(2.0, 28), 27);
        assert_eq!(slider_thumb_x(-1.0, 248.0, 29.0), 0.0);
        assert_eq!(slider_thumb_x(2.0, 248.0, 29.0), 219.0);
    }

    #[test]
    fn vertical_slider_helpers_clamp_values() {
        assert_eq!(vertical_slider_thumb_y(-1.0, 63.0, 11.0), 52.0);
        assert_eq!(vertical_slider_thumb_y(2.0, 63.0, 11.0), 0.0);
        assert_eq!(vertical_slider_thumb_y_down(-1.0, 145.0, 18.0), 0.0);
        assert_eq!(vertical_slider_thumb_y_down(2.0, 145.0, 18.0), 127.0);
    }

    #[test]
    fn winamp_windows_attach_by_touching_edges() {
        let main = Point::new(100.0, 200.0);
        let main_size = Size::new(275.0, 116.0);
        let child_size = Size::new(275.0, 116.0);

        assert!(rects_attached(
            Point::new(375.0, 200.0),
            child_size,
            main,
            main_size
        ));
        assert!(rects_attached(
            Point::new(101.0, 316.0),
            child_size,
            main,
            main_size
        ));
        assert!(!rects_attached(
            Point::new(420.0, 200.0),
            child_size,
            main,
            main_size
        ));
    }

    #[test]
    fn winamp_windows_do_not_attach_through_visible_snap_gap() {
        let main = Point::new(100.0, 200.0);
        let main_size = Size::new(275.0, 116.0);
        let child_size = Size::new(275.0, 116.0);

        assert!(!rects_attached(
            Point::new(379.0, 204.0),
            child_size,
            main,
            main_size
        ));
        assert_eq!(
            snap_candidate_deltas(
                WinampWindowSnapshot {
                    id: WinampWindowId::Equalizer,
                    position: Point::new(379.0, 204.0),
                    size: child_size,
                },
                WinampWindowSnapshot {
                    id: WinampWindowId::Main,
                    position: main,
                    size: main_size,
                },
            ),
            vec![Point::new(-4.0, 0.0)]
        );
        assert!(!rects_attached(
            Point::new(390.0, 204.0),
            child_size,
            main,
            main_size
        ));
    }

    #[test]
    fn winamp_attached_component_follows_pairwise_connections() {
        let snapshots = attached_chain_snapshots();

        assert_eq!(
            sorted_component(&snapshots, WinampWindowId::Main),
            vec![
                WinampWindowId::Main,
                WinampWindowId::Equalizer,
                WinampWindowId::Playlist
            ]
        );
        assert_eq!(
            sorted_component(&snapshots, WinampWindowId::Equalizer),
            vec![
                WinampWindowId::Main,
                WinampWindowId::Equalizer,
                WinampWindowId::Playlist
            ]
        );
        assert_eq!(
            sorted_component(&snapshots, WinampWindowId::Playlist),
            vec![
                WinampWindowId::Main,
                WinampWindowId::Equalizer,
                WinampWindowId::Playlist
            ]
        );
    }

    #[test]
    fn winamp_attached_component_handles_each_pair_topology() {
        let main_equalizer = [
            WinampWindowSnapshot {
                id: WinampWindowId::Main,
                position: Point::new(100.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Equalizer,
                position: Point::new(100.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Playlist,
                position: Point::new(700.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
        ];
        assert_eq!(
            sorted_component(&main_equalizer, WinampWindowId::Main),
            vec![WinampWindowId::Main, WinampWindowId::Equalizer]
        );
        assert_eq!(
            sorted_component(&main_equalizer, WinampWindowId::Playlist),
            vec![WinampWindowId::Playlist]
        );

        let equalizer_playlist = [
            WinampWindowSnapshot {
                id: WinampWindowId::Main,
                position: Point::new(100.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Equalizer,
                position: Point::new(500.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Playlist,
                position: Point::new(775.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
        ];
        assert_eq!(
            sorted_component(&equalizer_playlist, WinampWindowId::Equalizer),
            vec![WinampWindowId::Equalizer, WinampWindowId::Playlist]
        );
        assert_eq!(
            sorted_component(&equalizer_playlist, WinampWindowId::Main),
            vec![WinampWindowId::Main]
        );

        let main_playlist = [
            WinampWindowSnapshot {
                id: WinampWindowId::Main,
                position: Point::new(100.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Equalizer,
                position: Point::new(900.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Playlist,
                position: Point::new(375.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
        ];
        assert_eq!(
            sorted_component(&main_playlist, WinampWindowId::Playlist),
            vec![WinampWindowId::Main, WinampWindowId::Playlist]
        );
        assert_eq!(
            sorted_component(&main_playlist, WinampWindowId::Equalizer),
            vec![WinampWindowId::Equalizer]
        );
    }

    #[test]
    fn winamp_attached_component_leaves_all_separated_windows_out() {
        let snapshots = [
            WinampWindowSnapshot {
                id: WinampWindowId::Main,
                position: Point::new(100.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Equalizer,
                position: Point::new(500.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Playlist,
                position: Point::new(900.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
        ];

        assert_eq!(
            sorted_component(&snapshots, WinampWindowId::Main),
            vec![WinampWindowId::Main]
        );
        assert_eq!(
            sorted_component(&snapshots, WinampWindowId::Equalizer),
            vec![WinampWindowId::Equalizer]
        );
        assert_eq!(
            sorted_component(&snapshots, WinampWindowId::Playlist),
            vec![WinampWindowId::Playlist]
        );
    }

    #[test]
    fn winamp_move_snaps_separate_playlist_into_main_equalizer_component() {
        let snapshots = [
            WinampWindowSnapshot {
                id: WinampWindowId::Main,
                position: Point::new(100.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Equalizer,
                position: Point::new(100.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Playlist,
                position: Point::new(390.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
        ];

        assert_eq!(
            snap_candidate_deltas(
                WinampWindowSnapshot {
                    id: WinampWindowId::Equalizer,
                    position: Point::new(110.0, 216.0),
                    size: Size::new(275.0, 116.0),
                },
                snapshots[2],
            ),
            vec![Point::new(5.0, 0.0)]
        );

        let moved = move_winamp_snapshots(&snapshots, WinampWindowId::Main, Point::new(10.0, 0.0));

        assert_eq!(
            snapshot_position(&moved, WinampWindowId::Equalizer),
            Point::new(115.0, 216.0)
        );
        assert_eq!(
            snapshot_position(&moved, WinampWindowId::Playlist),
            Point::new(390.0, 216.0)
        );
        assert_eq!(
            sorted_component(&moved, WinampWindowId::Main),
            vec![
                WinampWindowId::Main,
                WinampWindowId::Equalizer,
                WinampWindowId::Playlist
            ]
        );
    }

    #[test]
    fn winamp_child_drag_snaps_without_moving_attached_neighbors() {
        let snapshots = [
            WinampWindowSnapshot {
                id: WinampWindowId::Main,
                position: Point::new(100.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Equalizer,
                position: Point::new(100.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Playlist,
                position: Point::new(386.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
        ];

        let mut component = vec![WinampWindowId::Playlist];
        let snapped = move_winamp_snapshots_for_component(
            &snapshots,
            &mut component,
            Point::new(-8.0, 0.0),
            false,
        );
        assert_eq!(
            snapshot_position(&snapped, WinampWindowId::Playlist),
            Point::new(375.0, 216.0)
        );
        assert_eq!(
            snapshot_position(&snapped, WinampWindowId::Main),
            Point::new(100.0, 100.0)
        );
        assert_eq!(
            snapshot_position(&snapped, WinampWindowId::Equalizer),
            Point::new(100.0, 216.0)
        );

        let moved_again = move_winamp_snapshots_for_component(
            &snapped,
            &mut component,
            Point::new(12.0, 7.0),
            false,
        );
        assert_eq!(
            snapshot_position(&moved_again, WinampWindowId::Main),
            Point::new(100.0, 100.0)
        );
        assert_eq!(
            snapshot_position(&moved_again, WinampWindowId::Equalizer),
            Point::new(100.0, 216.0)
        );
        assert_eq!(
            snapshot_position(&moved_again, WinampWindowId::Playlist),
            Point::new(387.0, 223.0)
        );
    }

    #[test]
    fn winamp_main_drag_session_keeps_initial_component_for_large_delta() {
        let snapshots = attached_chain_snapshots();
        let mut component = attached_winamp_component(&snapshots, WinampWindowId::Main);

        let moved = move_winamp_snapshots_for_component(
            &snapshots,
            &mut component,
            Point::new(220.0, 90.0),
            true,
        );

        assert_eq!(
            snapshot_position(&moved, WinampWindowId::Equalizer),
            Point::new(320.0, 306.0)
        );
        assert_eq!(
            snapshot_position(&moved, WinampWindowId::Playlist),
            Point::new(595.0, 306.0)
        );
        assert_eq!(
            sorted_component(&moved, WinampWindowId::Main),
            vec![
                WinampWindowId::Main,
                WinampWindowId::Equalizer,
                WinampWindowId::Playlist
            ]
        );
    }

    fn attached_chain_snapshots() -> [WinampWindowSnapshot; 3] {
        [
            WinampWindowSnapshot {
                id: WinampWindowId::Main,
                position: Point::new(100.0, 100.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Equalizer,
                position: Point::new(100.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
            WinampWindowSnapshot {
                id: WinampWindowId::Playlist,
                position: Point::new(375.0, 216.0),
                size: Size::new(275.0, 116.0),
            },
        ]
    }

    fn sorted_component(
        snapshots: &[WinampWindowSnapshot],
        dragged: WinampWindowId,
    ) -> Vec<WinampWindowId> {
        let mut component = attached_winamp_component(snapshots, dragged);
        component.sort_by_key(|id| *id as u8);
        component
    }

    fn snapshot_position(snapshots: &[WinampWindowSnapshot], id: WinampWindowId) -> Point {
        snapshots
            .iter()
            .find(|snapshot| snapshot.id == id)
            .expect("snapshot")
            .position
    }

    fn snap_candidate_deltas(
        moving: WinampWindowSnapshot,
        stationary: WinampWindowSnapshot,
    ) -> Vec<Point> {
        winamp_snap_candidates(moving, stationary)
            .into_iter()
            .map(|candidate| candidate.delta)
            .collect()
    }
}
