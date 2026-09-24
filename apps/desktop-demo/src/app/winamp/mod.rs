mod skin;
pub(crate) mod sprites;

use std::rc::Rc;

use cranpose::{rememberWindowState, WindowConfig, WindowModifierExt, WindowState};
use cranpose_core::{self, MutableState};
use cranpose_foundation::PointerButton;
use cranpose_ui::{
    composable, current_density, Box, BoxSpec, Button, ButtonSpec, Canvas, Color, Column,
    ColumnSpec, Modifier, Point, PointerEventKind, PointerInputScope, Size, Text, TextStyle,
};
use cranpose_ui_graphics::{ImageBitmap, Rect};
use skin::{load_skin, WinampSkin};
use sprites::*;

fn winamp_press_debug_enabled() -> bool {
    std::env::var_os("WINAMP_PRESS_DEBUG").is_some()
}

fn winamp_native_trace_enabled() -> bool {
    std::env::var_os("CRANPOSE_NATIVE_TRACE").is_some()
}

fn trace_winamp_state(action: &str, state: &WinampState) {
    if winamp_native_trace_enabled() {
        println!(
            "winamp trace: action={action} closed={} playback={:?} eq_visible={} playlist_visible={} volume={:.3} status={:?}",
            state.closed,
            state.playback,
            state.eq_visible,
            state.playlist_visible,
            state.volume,
            state.status
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
struct WinampState {
    closed: bool,
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
            closed: false,
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
    Native,
    Tearable(WinampStackPane),
    Dockable(WinampStackPane),
}

impl WinampDragTarget {
    fn pane_window(self) -> Option<WindowState> {
        match self {
            Self::Tearable(stacked) | Self::Dockable(stacked) => Some(stacked.state),
            Self::Inline(_) | Self::Native => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct WinampStackPane {
    pane: WinampPane,
    dock: MutableState<WinampDock>,
    offset: Point,
    main_window: WindowState,
    state: WindowState,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum WinampPane {
    Equalizer,
    Playlist,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct WinampDock {
    torn: Vec<WinampPane>,
}

impl WinampDock {
    fn docked(&self, pane: WinampPane) -> bool {
        !self.torn.contains(&pane)
    }

    fn tear(&mut self, pane: WinampPane) {
        if self.docked(pane) {
            self.torn.push(pane);
        }
    }

    fn dock(&mut self, pane: WinampPane) {
        self.torn.retain(|held| *held != pane);
    }
}

const WINAMP_TEAR_REACH: f32 = 18.0;
const WINAMP_DOCK_REACH: f32 = 24.0;

fn pane_left_the_stack(travel: Point) -> bool {
    travel.x.abs() > WINAMP_TEAR_REACH || travel.y.abs() > WINAMP_TEAR_REACH
}

fn pane_came_back_to_its_slot(origin: Point, slot: Point) -> bool {
    (origin.y - slot.y).abs() <= WINAMP_DOCK_REACH && (origin.x - slot.x).abs() <= MAIN_WIDTH / 2.0
}

fn dock_a_pane_let_go_on_the_stack(torn: WinampStackPane) {
    let Some(origin) = torn.state.position_non_reactive() else {
        return;
    };
    let Some(home) = torn.main_window.position_non_reactive() else {
        return;
    };
    let slot = Point::new(home.x, home.y + torn.main_window.size_non_reactive().height);
    if pane_came_back_to_its_slot(origin, slot) {
        torn.dock.update(|held| held.dock(torn.pane));
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct WinampPaneSlot {
    docked: bool,
    offset: Point,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct WinampStackLayout {
    equalizer: Option<WinampPaneSlot>,
    playlist: Option<WinampPaneSlot>,
    size: Size,
}

fn winamp_stack_layout(
    snapshot: &WinampState,
    dock: &WinampDock,
    playlist: Size,
) -> WinampStackLayout {
    let mut size = Size::new(MAIN_WIDTH, MAIN_HEIGHT);
    let equalizer = snapshot.eq_visible.then_some(WinampPaneSlot {
        docked: dock.docked(WinampPane::Equalizer),
        offset: Point::new(0.0, size.height),
    });
    if equalizer.is_some_and(|slot| slot.docked) {
        size.height += EQ_HEIGHT;
    }
    let playlist_slot = snapshot.playlist_visible.then_some(WinampPaneSlot {
        docked: dock.docked(WinampPane::Playlist),
        offset: Point::new(0.0, size.height),
    });
    if playlist_slot.is_some_and(|slot| slot.docked) {
        size.width = size.width.max(playlist.width);
        size.height += playlist.height;
    }
    WinampStackLayout {
        equalizer,
        playlist: playlist_slot,
        size,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum WinampCloseAction {
    SetStatus,
    CloseApp,
}

const MAIN_TITLE_DRAG_HIT_AREA: SpriteRect = (16.0, 0.0, 228.0, 14.0);
const EQ_TITLE_DRAG_HIT_AREA: SpriteRect = (0.0, 0.0, 264.0, 14.0);
const WINAMP_MAIN_TITLE: &str = "Winamp";
const WINAMP_EQUALIZER_TITLE: &str = "Winamp Equalizer";
const WINAMP_PLAYLIST_TITLE: &str = "Winamp Playlist";

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct WinampTabState {
    player: MutableState<WinampState>,
    detached: MutableState<bool>,
    dock: MutableState<WinampDock>,
    inline_windows: WinampInlineWindowStates,
    peer_windows: WinampPeerWindowStates,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct WinampInlineWindowStates {
    main: MutableState<Point>,
    equalizer: MutableState<Point>,
    playlist: MutableState<Point>,
}

impl WinampInlineWindowStates {
    fn of(self, pane: WinampPane) -> MutableState<Point> {
        match pane {
            WinampPane::Equalizer => self.equalizer,
            WinampPane::Playlist => self.playlist,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct WinampPeerWindowStates {
    main: WindowState,
    equalizer: WindowState,
    playlist: WindowState,
}

impl WinampPeerWindowStates {
    fn of(self, pane: WinampPane) -> WindowState {
        match pane {
            WinampPane::Equalizer => self.equalizer,
            WinampPane::Playlist => self.playlist,
        }
    }
}

#[derive(Clone, Copy)]
struct WinampWindowPlacement {
    title: &'static str,
    initial_position: WinampInitialWindowPosition,
    state: WindowState,
}

#[derive(Clone, Copy, PartialEq)]
enum WinampInitialWindowPosition {
    Host(Point),
    Screen(Point),
}

#[derive(Clone, Copy, PartialEq)]
struct WinampWindowPlaces {
    main: WinampInitialWindowPosition,
    equalizer: WinampInitialWindowPosition,
    playlist: WinampInitialWindowPosition,
}

#[derive(Clone, Copy, PartialEq)]
struct WinampNativeWindows {
    peers: WinampPeerWindowStates,
    places: WinampWindowPlaces,
}

impl WinampNativeWindows {
    fn pane_config(self, pane: WinampPane, scale: f32) -> WindowConfig {
        match pane {
            WinampPane::Equalizer => winamp_window_config(WinampWindowPlacement {
                title: WINAMP_EQUALIZER_TITLE,
                initial_position: self.places.equalizer,
                state: self.peers.equalizer,
            }),
            WinampPane::Playlist => winamp_window_config(WinampWindowPlacement {
                title: WINAMP_PLAYLIST_TITLE,
                initial_position: self.places.playlist,
                state: self.peers.playlist,
            })
            .with_min_size(
                scaled(PLAYLIST_WIDTH, scale),
                scaled(PLAYLIST_HEIGHT, scale),
            ),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum WinampStageWindows {
    Inline(WinampInlineWindowStates),
    Native(WinampNativeWindows),
}

#[derive(Clone, Copy, PartialEq)]
struct WinampPanePlace {
    pane: WinampPane,
    slot: WinampPaneSlot,
    dock: MutableState<WinampDock>,
}

impl WinampStageWindows {
    fn playlist_window(self) -> Option<WindowState> {
        match self {
            Self::Inline(_) => None,
            Self::Native(native) => Some(native.peers.playlist),
        }
    }

    fn stack_modifier(self) -> Modifier {
        match self {
            Self::Inline(_) => Modifier::empty()
                .fill_max_size()
                .clip_to_bounds()
                .background(Color(0.02, 0.02, 0.03, 1.0))
                .rounded_corners(8.0),
            Self::Native(native) => {
                Modifier::empty().window(winamp_window_config(WinampWindowPlacement {
                    title: WINAMP_MAIN_TITLE,
                    initial_position: native.places.main,
                    state: native.peers.main,
                }))
            }
        }
    }

    fn main_drag_target(self) -> WinampDragTarget {
        match self {
            Self::Inline(inline) => WinampDragTarget::Inline(inline.main),
            Self::Native(_) => WinampDragTarget::Native,
        }
    }

    fn pane_modifier(self, place: WinampPanePlace, scale: f32) -> Modifier {
        match self {
            Self::Native(native) if !place.slot.docked => {
                Modifier::empty().window(native.pane_config(place.pane, scale))
            }
            Self::Inline(_) | Self::Native(_) => Modifier::empty(),
        }
    }

    fn pane_drag_target(self, place: WinampPanePlace) -> WinampDragTarget {
        let native = match self {
            Self::Inline(inline) => return WinampDragTarget::Inline(inline.of(place.pane)),
            Self::Native(native) => native,
        };
        let stacked = WinampStackPane {
            pane: place.pane,
            dock: place.dock,
            offset: place.slot.offset,
            main_window: native.peers.main,
            state: native.peers.of(place.pane),
        };
        if place.slot.docked {
            WinampDragTarget::Tearable(stacked)
        } else {
            WinampDragTarget::Dockable(stacked)
        }
    }
}

#[derive(Clone, PartialEq)]
struct WinampStageSpec {
    skin: WinampSkin,
    state: MutableState<WinampState>,
    dock: MutableState<WinampDock>,
    windows: WinampStageWindows,
    close: WinampCloseAction,
    scale: f32,
}

#[composable]
pub(crate) fn remember_winamp_tab_state() -> WinampTabState {
    cranpose_core::remember(|| WinampTabState {
        player: cranpose_core::mutableStateOf(WinampState::default()),
        detached: cranpose_core::mutableStateOf(native_winamp_windows_available()),
        dock: cranpose_core::mutableStateOf(WinampDock::default()),
        inline_windows: WinampInlineWindowStates {
            main: cranpose_core::mutableStateOf(Point::new(26.0, 22.0)),
            equalizer: cranpose_core::mutableStateOf(Point::new(26.0, 142.0)),
            playlist: cranpose_core::mutableStateOf(Point::new(336.0, 22.0)),
        },
        peer_windows: WinampPeerWindowStates {
            main: WindowState::new(MAIN_WIDTH, MAIN_HEIGHT),
            equalizer: WindowState::new(EQ_WIDTH, EQ_HEIGHT),
            playlist: WindowState::new(PLAYLIST_WIDTH, PLAYLIST_HEIGHT),
        },
    })
    .with(|state| *state)
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
    let inline = tab_state.inline_windows;
    let windows = if detached {
        WinampStageWindows::Native(WinampNativeWindows {
            peers: tab_state.peer_windows,
            places: WinampWindowPlaces {
                main: WinampInitialWindowPosition::Host(inline.main.get()),
                equalizer: WinampInitialWindowPosition::Host(inline.equalizer.get()),
                playlist: WinampInitialWindowPosition::Host(inline.playlist.get()),
            },
        })
    } else {
        WinampStageWindows::Inline(inline)
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

            WinampStage(WinampStageSpec {
                skin: skin.clone(),
                state,
                dock: tab_state.dock,
                windows,
                close: WinampCloseAction::SetStatus,
                scale,
            });
        },
    );
}

fn remember_winamp_skin() -> Result<WinampSkin, String> {
    cranpose_core::remember(|| {
        let wsz = include_bytes!("../../../assets/winamp.wsz");
        load_skin(wsz).map_err(|err| format!("{err:#}"))
    })
    .with(Clone::clone)
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
        ButtonSpec::default(),
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
fn WinampStage(spec: WinampStageSpec) {
    let WinampStageSpec {
        skin,
        state,
        dock,
        windows,
        close,
        scale,
    } = spec;
    let snapshot = state.get();
    let playlist_size = playlist_skin_size(playlist_window_size(windows.playlist_window()), scale);
    let layout = winamp_stack_layout(&snapshot, &dock.get(), playlist_size);
    if let WinampStageWindows::Native(native) = windows {
        let size = Size::new(
            scaled(layout.size.width, scale),
            scaled(layout.size.height, scale),
        );
        if native.peers.main.size_non_reactive() != size {
            native.peers.main.set_size(size);
        }
    }

    Box(windows.stack_modifier(), BoxSpec::default(), move || {
        MainWindow(
            skin.clone(),
            state,
            windows.main_drag_target(),
            close,
            scale,
        );
        if let Some(slot) = layout.equalizer {
            let place = WinampPanePlace {
                pane: WinampPane::Equalizer,
                slot,
                dock,
            };
            let skin = skin.clone();
            Box(
                windows.pane_modifier(place, scale),
                BoxSpec::default(),
                move || {
                    EqualizerWindow(skin.clone(), state, windows.pane_drag_target(place), scale);
                },
            );
        }
        if let Some(slot) = layout.playlist {
            let place = WinampPanePlace {
                pane: WinampPane::Playlist,
                slot,
                dock,
            };
            let pledit = skin.pledit.clone();
            Box(
                windows.pane_modifier(place, scale),
                BoxSpec::default(),
                move || {
                    PlaylistWindow(
                        pledit.clone(),
                        state,
                        windows.pane_drag_target(place),
                        playlist_size,
                        scale,
                    );
                },
            );
        }
    });
}

#[composable]
pub fn WinampStandaloneApp() {
    let state = cranpose_core::rememberMutableStateOf(WinampState::default);
    let dock = cranpose_core::rememberMutableStateOf(WinampDock::default);
    let peers = WinampPeerWindowStates {
        main: rememberWindowState(MAIN_WIDTH, MAIN_HEIGHT),
        equalizer: rememberWindowState(EQ_WIDTH, EQ_HEIGHT),
        playlist: rememberWindowState(PLAYLIST_WIDTH, PLAYLIST_HEIGHT),
    };
    if state.get().closed {
        return;
    }
    let skin = match remember_winamp_skin() {
        Ok(skin) => skin,
        Err(error) => {
            WinampSkinError(error);
            return;
        }
    };

    WinampStage(WinampStageSpec {
        skin,
        state,
        dock,
        windows: WinampStageWindows::Native(WinampNativeWindows {
            peers,
            places: WinampWindowPlaces {
                main: WinampInitialWindowPosition::Screen(Point::new(140.0, 120.0)),
                equalizer: WinampInitialWindowPosition::Screen(Point::new(
                    140.0,
                    120.0 + MAIN_HEIGHT,
                )),
                playlist: WinampInitialWindowPosition::Screen(Point::new(
                    140.0 + EQ_WIDTH,
                    120.0 + MAIN_HEIGHT,
                )),
            },
        }),
        close: WinampCloseAction::CloseApp,
        scale: ui_scale(),
    });
}

#[composable]
fn MainWindow(
    skin: WinampSkin,
    state: MutableState<WinampState>,
    drag_target: WinampDragTarget,
    close_action: WinampCloseAction,
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

            WindowDragHandle(drag_target, MAIN_TITLE_DRAG_HIT_AREA, scale);

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
                        state_click.update(|s| match close_action {
                            WinampCloseAction::SetStatus => {
                                s.status = "Close".to_string();
                                trace_winamp_state("main-close-status", s);
                            }
                            WinampCloseAction::CloseApp => {
                                s.closed = true;
                                s.status = "Closed".to_string();
                                trace_winamp_state("main-close-app", s);
                            }
                        });
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
                        state_drag.update(|s| {
                            s.volume = fraction;
                            trace_winamp_state("volume", s);
                        });
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
                            trace_winamp_state("main-eq-toggle", s);
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
                            trace_winamp_state("main-playlist-toggle", s);
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

            WindowDragHandle(drag_target, EQ_TITLE_DRAG_HIT_AREA, scale);

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
                            trace_winamp_state("eq-close", s);
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
    size: Size,
    scale: f32,
) {
    let snapshot = state.get();
    let Size { width, height } = size;
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
            PlaylistResizeHandle(
                drag_target,
                (width - 16.0, height - 16.0, 16.0, 16.0),
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
    let is_pressed = cranpose_core::rememberMutableStateOf(|| false);
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
                                                        "[WINAMP_PRESS_DEBUG] move-clears button ({x:.1},{y:.1})"
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
                                                        "[WINAMP_PRESS_DEBUG] click fired button ({x:.1},{y:.1})"
                                                    );
                                                }
                                                on_click();
                                            }
                                            event.consume();
                                        }
                                        PointerEventKind::Cancel => {
                                            if winamp_press_debug_enabled() {
                                                eprintln!(
                                                    "[WINAMP_PRESS_DEBUG] cancel button ({x:.1},{y:.1})"
                                                );
                                            }
                                            is_pressed.set(false);
                                        }
                                        PointerEventKind::Scroll
                                        | PointerEventKind::Zoom
                                        | PointerEventKind::RotaryScrollPre
                                        | PointerEventKind::RotaryScroll
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
    let modifier = match drag_target {
        WinampDragTarget::Native => modifier.window_drag_area(|| {}, || {}),
        WinampDragTarget::Dockable(stacked) => {
            modifier.window_drag_area(|| {}, move || dock_a_pane_let_go_on_the_stack(stacked))
        }
        WinampDragTarget::Tearable(stacked) => tear_grip(modifier, stacked, scale),
        WinampDragTarget::Inline(window_position) => inline_drag(modifier, window_position),
    };
    Box(modifier, BoxSpec::default(), || {});
}

fn tear_grip(modifier: Modifier, stacked: WinampStackPane, scale: f32) -> Modifier {
    let key = (
        stacked.pane,
        stacked.offset.x.to_bits(),
        stacked.offset.y.to_bits(),
    );
    modifier.pointer_input(key, move |scope: PointerInputScope| async move {
        scope
            .await_pointer_event_scope(|await_scope| async move {
                let mut pressed_at = None::<Point>;
                loop {
                    let event = await_scope.await_pointer_event().await;
                    match event.kind {
                        PointerEventKind::Down => {
                            pressed_at = Some(event.global_position);
                            event.consume();
                        }
                        PointerEventKind::Move => {
                            let Some(start) = pressed_at else {
                                continue;
                            };
                            let travel = Point::new(
                                event.global_position.x - start.x,
                                event.global_position.y - start.y,
                            );
                            if pane_left_the_stack(travel) {
                                pressed_at = None;
                                tear_out(stacked, travel, scale);
                            }
                        }
                        PointerEventKind::Up | PointerEventKind::Cancel => pressed_at = None,
                        _ => {}
                    }
                }
            })
            .await;
    })
}

fn tear_out(stacked: WinampStackPane, travel: Point, scale: f32) {
    let home = stacked
        .main_window
        .position_non_reactive()
        .unwrap_or(Point::new(0.0, 0.0));
    stacked.state.set_position(Some(Point::new(
        snap_to_pixel(home.x + scaled(stacked.offset.x, scale) + travel.x),
        snap_to_pixel(home.y + scaled(stacked.offset.y, scale) + travel.y),
    )));
    stacked.dock.update(|held| held.tear(stacked.pane));
}

fn inline_drag(modifier: Modifier, window_position: MutableState<Point>) -> Modifier {
    modifier.pointer_input((), move |scope: PointerInputScope| async move {
        scope
            .await_pointer_event_scope(|await_scope| async move {
                let mut grabbed = None::<Point>;
                loop {
                    let event = await_scope.await_pointer_event().await;
                    match event.kind {
                        PointerEventKind::Down => {
                            let current = window_position.get();
                            grabbed = Some(Point::new(
                                event.global_position.x - current.x,
                                event.global_position.y - current.y,
                            ));
                            event.consume();
                        }
                        PointerEventKind::Move => {
                            if !event.buttons.contains(PointerButton::Primary) {
                                grabbed = None;
                                continue;
                            }
                            if let Some(offset) = grabbed {
                                window_position.set(Point::new(
                                    snap_to_pixel(event.global_position.x - offset.x),
                                    snap_to_pixel(event.global_position.y - offset.y),
                                ));
                                event.consume();
                            }
                        }
                        PointerEventKind::Up | PointerEventKind::Cancel => grabbed = None,
                        _ => {}
                    }
                }
            })
            .await;
    })
}

#[composable]
fn PlaylistResizeHandle(drag_target: WinampDragTarget, area: SpriteRect, scale: f32) {
    let Some(window) = drag_target.pane_window() else {
        return;
    };
    Box(
        Modifier::empty()
            .size_points(scaled(area.2, scale), scaled(area.3, scale))
            .absolute_offset(scaled(area.0, scale), scaled(area.1, scale))
            .pointer_input((), move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        let mut grabbed = None::<(Point, Size)>;
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Down => {
                                    grabbed =
                                        Some((event.global_position, window.size_non_reactive()));
                                    event.consume();
                                }
                                PointerEventKind::Move => {
                                    let Some((grabbed_at, held)) = grabbed else {
                                        continue;
                                    };
                                    window.set_size(stretched_playlist(
                                        held,
                                        Point::new(
                                            event.global_position.x - grabbed_at.x,
                                            event.global_position.y - grabbed_at.y,
                                        ),
                                        scale,
                                    ));
                                    event.consume();
                                }
                                PointerEventKind::Up | PointerEventKind::Cancel => grabbed = None,
                                _ => {}
                            }
                        }
                    })
                    .await;
            }),
        BoxSpec::default(),
        || {},
    );
}

fn stretched_playlist(held: Size, travel: Point, scale: f32) -> Size {
    Size::new(
        snap_to_pixel((held.width + travel.x).max(scaled(PLAYLIST_WIDTH, scale))),
        snap_to_pixel((held.height + travel.y).max(scaled(PLAYLIST_HEIGHT, scale))),
    )
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
                    trace_winamp_state("play", s);
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
                    trace_winamp_state("pause", s);
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
                    trace_winamp_state("stop", s);
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
    let config = WindowConfig::borderless(placement.title, state_size.width, state_size.height);
    let config = match placement.initial_position {
        WinampInitialWindowPosition::Host(position) => config.with_host_window_position(
            snap_to_pixel(position.x + WINAMP_NATIVE_HOST_OFFSET_X),
            snap_to_pixel(position.y + WINAMP_NATIVE_HOST_OFFSET_Y),
        ),
        WinampInitialWindowPosition::Screen(position) => {
            config.with_position(snap_to_pixel(position.x), snap_to_pixel(position.y))
        }
    };
    config
        .with_transparent(false)
        .with_resizable(false)
        .with_visible(true)
}

fn winamp_window_config(placement: WinampWindowPlacement) -> WindowConfig {
    let state = placement.state;
    base_winamp_window_config(placement).with_state(state)
}

pub(crate) fn playlist_window_size(window: Option<WindowState>) -> Size {
    window.map_or_else(
        || Size::new(PLAYLIST_WIDTH, PLAYLIST_HEIGHT),
        WindowState::size,
    )
}

fn playlist_skin_size(window_size: Size, scale: f32) -> Size {
    let skin_scale = scale.max(f32::EPSILON);
    Size::new(
        (window_size.width / skin_scale).max(PLAYLIST_WIDTH),
        (window_size.height / skin_scale).max(PLAYLIST_HEIGHT),
    )
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
        WinampDragTarget::Native | WinampDragTarget::Dockable(_) => modifier,
        WinampDragTarget::Tearable(stacked) => modifier.offset(
            scaled(stacked.offset.x, scale),
            scaled(stacked.offset.y, scale),
        ),
    }
}

fn ui_scale() -> f32 {
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
#[path = "tests/winamp_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/dock_tests.rs"]
mod dock_tests;
