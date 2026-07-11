use cranpose_animation::{
    animateFloatAsState, infiniteRepeatable, rememberInfiniteTransition, AnimationSpec,
    AnimationType, RepeatMode, StartOffset,
};
use cranpose_core::{
    self, compositionLocalOf, CompositionLocal, CompositionLocalProvider, DisposableEffect,
    DisposableEffectResult, LaunchedEffect, MutableState,
};
use cranpose_foundation::lazy::LazyListState;
use cranpose_foundation::text::TextFieldState;
use cranpose_foundation::PointerEventKind;
use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::{
    composable, BasicTextField, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec,
    CornerRadii, GraphicsLayer, IntrinsicSize, LinearArrangement, Modifier, Point,
    PointerInputScope, RoundedCornerShape, Row, RowSpec, Size, Spacer, Text, TextStyle,
    VerticalAlignment,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

mod animations;
mod hacker_news;
mod images;
mod interactive_anim;
pub mod lazy_list;
mod lazy_scrollbar;
mod liquid_ui;
mod markdown;
mod mineswapper2;
mod shader_rect;
mod shaders;
mod text_showcase;
mod web_fetch;
mod winamp;
mod xkcd;

use animations::AnimationsTab;
use hacker_news::{HackerNewsScrollStabilityFixtureTab, HackerNewsTab};
use images::images_tab;
use interactive_anim::InteractiveAnimTab;
use lazy_list::lazy_list_example;
use liquid_ui::LiquidUiTab;
use markdown::{
    markdown_viewer_tab, MarkdownScrollStabilityFixtureTab, MarkdownScrollStressFixtureTab,
    MarkdownScrollStressFixtureTabWithState,
};
use shader_rect::ShaderRectTab;
pub use shaders::ShaderSection;
use shaders::ShadersTab;
use text_showcase::TextShowcaseTab;
use web_fetch::web_fetch_example;
pub use winamp::WinampStandaloneApp;
use winamp::{remember_winamp_tab_state, WinampTab, WinampTabState};
use xkcd::xkcd_tab;

pub use hacker_news::HACKER_NEWS_SCROLL_STABILITY_TARGET_TITLE;
pub use markdown::{markdown_scroll_stress_fixture, MARKDOWN_SCROLL_STABILITY_TARGET_TEXT};

thread_local! {
    pub static TEST_COMPOSITION_LOCAL_COUNTER: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    pub static TEST_ACTIVE_TAB_STATE: RefCell<Option<MutableState<DemoTab>>> = const { RefCell::new(None) };
    pub static TEST_COUNTER_APP_COUNTER_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    pub static TEST_COUNTER_APP_POINTER_DOWN_STATE: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
    pub static TEST_COUNTER_APP_POINTER_POSITION_STATE: RefCell<Option<MutableState<Point>>> = const { RefCell::new(None) };
    pub static TEST_RECURSIVE_LAYOUT_DEPTH_STATE: RefCell<Option<MutableState<usize>>> = const { RefCell::new(None) };
    pub static TEST_LAZY_LIST_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DemoTab {
    Counter,
    CompositionLocal,
    Async,
    Animations,
    InteractiveAnim,
    WebFetch,
    TextInput,
    Layout,
    ModifierShowcase,
    LazyList,
    Mineswapper2,
    HackerNews,
    Images,
    Text,
    Winamp,
    Xkcd,
    Shaders,
    ShaderRect,
    Liquid,
    MarkdownViewer,
    FilePicker,
}

impl DemoTab {
    pub fn label(self) -> &'static str {
        match self {
            DemoTab::Counter => "Counter App",
            DemoTab::CompositionLocal => "CompositionLocal Test",
            DemoTab::Async => "Async Runtime",
            DemoTab::Animations => "Animations",
            DemoTab::InteractiveAnim => "Interactive Anim",
            DemoTab::WebFetch => "Web Fetch",
            DemoTab::TextInput => "Text Input",
            DemoTab::Layout => "Recursive Layout",
            DemoTab::ModifierShowcase => "Modifiers Showcase",
            DemoTab::LazyList => "Lazy List",
            DemoTab::Mineswapper2 => "Mineswapper2",
            DemoTab::HackerNews => "Hacker News",
            DemoTab::Images => "Images",
            DemoTab::Text => "Text",
            DemoTab::Winamp => "Winamp",
            DemoTab::Xkcd => "XKCD",
            DemoTab::Shaders => "Shaders",
            DemoTab::ShaderRect => "Shader Rect",
            DemoTab::Liquid => "Liquid UI",
            DemoTab::MarkdownViewer => "Markdown",
            DemoTab::FilePicker => "File Picker",
        }
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    pub fn from_startup_name(name: &str) -> Option<Self> {
        let normalized = name
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .map(|ch| ch.to_ascii_lowercase())
            .collect::<String>();
        match normalized.as_str() {
            "counter" | "counterapp" => Some(Self::Counter),
            "compositionlocal" | "compositionlocaltest" => Some(Self::CompositionLocal),
            "async" | "asyncruntime" => Some(Self::Async),
            "animations" => Some(Self::Animations),
            "interactiveanim" | "interactiveanimation" | "interactiveanimations" => {
                Some(Self::InteractiveAnim)
            }
            "webfetch" => Some(Self::WebFetch),
            "textinput" => Some(Self::TextInput),
            "layout" | "recursivelayout" => Some(Self::Layout),
            "modifiers" | "modifiersshowcase" | "modifiershowcase" => Some(Self::ModifierShowcase),
            "lazylist" => Some(Self::LazyList),
            "mineswapper2" => Some(Self::Mineswapper2),
            "hackernews" => Some(Self::HackerNews),
            "images" => Some(Self::Images),
            "text" => Some(Self::Text),
            "winamp" => Some(Self::Winamp),
            "xkcd" => Some(Self::Xkcd),
            "shaders" => Some(Self::Shaders),
            "shaderrect" => Some(Self::ShaderRect),
            "markdown" | "markdownviewer" => Some(Self::MarkdownViewer),
            "liquid" | "liquidui" => Some(Self::Liquid),
            _ => None,
        }
    }
}

pub const DEMO_TABS: [DemoTab; 21] = [
    DemoTab::Counter,
    DemoTab::CompositionLocal,
    DemoTab::Async,
    DemoTab::Animations,
    DemoTab::WebFetch,
    DemoTab::TextInput,
    DemoTab::Layout,
    DemoTab::ModifierShowcase,
    DemoTab::LazyList,
    DemoTab::Mineswapper2,
    DemoTab::HackerNews,
    DemoTab::Images,
    DemoTab::Text,
    DemoTab::Winamp,
    DemoTab::Xkcd,
    DemoTab::Shaders,
    DemoTab::ShaderRect,
    DemoTab::Liquid,
    DemoTab::MarkdownViewer,
    DemoTab::InteractiveAnim,
    DemoTab::FilePicker,
];

pub fn demo_tab_labels() -> Vec<&'static str> {
    DEMO_TABS.iter().map(|tab| tab.label()).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StartupSelection {
    pub initial_tab: Option<DemoTab>,
    pub initial_shader_section: Option<ShaderSection>,
}

impl StartupSelection {
    #[cfg(any(test, target_arch = "wasm32"))]
    pub fn from_requested(
        initial_tab: Option<DemoTab>,
        initial_shader_section: Option<ShaderSection>,
    ) -> Self {
        match (initial_tab, initial_shader_section) {
            (Some(DemoTab::Shaders), initial_shader_section) => Self {
                initial_tab: Some(DemoTab::Shaders),
                initial_shader_section,
            },
            (Some(tab), Some(_)) => Self {
                initial_tab: Some(tab),
                initial_shader_section: None,
            },
            (Some(tab), None) => Self {
                initial_tab: Some(tab),
                initial_shader_section: None,
            },
            (None, Some(section)) => Self {
                initial_tab: Some(DemoTab::Shaders),
                initial_shader_section: Some(section),
            },
            (None, None) => Self::default(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Holder {
    count: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct AnimationState {
    pub progress: f32,
    pub direction: f32,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self {
            progress: 0.0,
            direction: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameStats {
    pub frames: u32,
    pub last_frame_ms: f32,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            frames: 0,
            last_frame_ms: 0.0,
        }
    }
}

fn local_holder() -> CompositionLocal<Holder> {
    thread_local! {
        static LOCAL_HOLDER: RefCell<Option<CompositionLocal<Holder>>> = const { RefCell::new(None) };
    }
    LOCAL_HOLDER.with(|cell| {
        let mut opt = cell.borrow_mut();
        opt.get_or_insert_with(|| compositionLocalOf(|| Holder { count: 0 }))
            .clone()
    })
}

fn random() -> i32 {
    (demo_random_u32() % 10000) as i32
}

fn demo_random_u32() -> u32 {
    let mut buf = [0u8; 4];
    match getrandom::fill(&mut buf) {
        Ok(()) => u32::from_le_bytes(buf),
        Err(_) => fallback_demo_random_u32(),
    }
}

fn fallback_demo_random_u32() -> u32 {
    thread_local! {
        static FALLBACK_RANDOM_STATE: Cell<u32> = const { Cell::new(0x9E37_79B9) };
    }

    FALLBACK_RANDOM_STATE.with(|state| {
        let next = state.get().wrapping_add(0x9E37_79B9);
        state.set(next);
        next ^ next.rotate_left(13) ^ next.rotate_right(9)
    })
}

fn cached_recursive_static_text(value: &'static str) -> Rc<cranpose_ui::text::AnnotatedString> {
    thread_local! {
        static TEXTS: RefCell<std::collections::HashMap<&'static str, Rc<cranpose_ui::text::AnnotatedString>>> =
            RefCell::new(std::collections::HashMap::new());
    }

    TEXTS.with(|texts| {
        let mut texts = texts.borrow_mut();
        texts
            .entry(value)
            .or_insert_with(|| Rc::new(cranpose_ui::text::AnnotatedString::from(value)))
            .clone()
    })
}

fn cached_depth_text(depth: usize) -> Rc<cranpose_ui::text::AnnotatedString> {
    thread_local! {
        static TEXTS: RefCell<Vec<Rc<cranpose_ui::text::AnnotatedString>>> = const { RefCell::new(Vec::new()) };
    }

    TEXTS.with(|texts| {
        let mut texts = texts.borrow_mut();
        while texts.len() <= depth {
            let next_depth = texts.len();
            texts.push(Rc::new(cranpose_ui::text::AnnotatedString::from(format!(
                "Depth {next_depth}"
            ))));
        }
        texts[depth].clone()
    })
}

fn cached_current_depth_text(depth: usize) -> Rc<cranpose_ui::text::AnnotatedString> {
    thread_local! {
        static TEXTS: RefCell<Vec<Rc<cranpose_ui::text::AnnotatedString>>> = const { RefCell::new(Vec::new()) };
    }

    TEXTS.with(|texts| {
        let mut texts = texts.borrow_mut();
        while texts.len() <= depth {
            let next_depth = texts.len();
            texts.push(Rc::new(cranpose_ui::text::AnnotatedString::from(format!(
                "Current depth: {next_depth}"
            ))));
        }
        texts[depth].clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub(crate) fn ScrollableTab(content: impl FnMut() + 'static) {
    let scroll_state =
        cranpose_core::remember(|| cranpose_ui::ScrollState::new(0.0)).with(|s| s.clone());
    let modifier = Modifier::empty()
        .fill_max_size()
        .vertical_scroll(scroll_state, false);
    Column(
        modifier,
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(16.0)),
        content,
    );
}

#[allow(non_snake_case)]
#[composable]
fn TabButton(tab: DemoTab, active_tab: cranpose_core::MutableState<DemoTab>, padding: f32) {
    let is_active = active_tab.get() == tab;
    Button(
        Modifier::empty()
            .rounded_corners(12.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    Brush::solid(if is_active {
                        Color(0.2, 0.45, 0.9, 1.0)
                    } else {
                        Color(0.3, 0.3, 0.3, 0.5)
                    }),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(padding),
        ButtonSpec::default(),
        {
            move || {
                if active_tab.get() != tab {
                    active_tab.set(tab);
                }
            }
        },
        {
            let label = tab.label();
            move || {
                Text(label, Modifier::empty().padding(4.0), TextStyle::default());
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn TabBarHorizontal(active_tab: cranpose_core::MutableState<DemoTab>) {
    let tabs_scroll_state =
        cranpose_core::remember(|| cranpose_ui::ScrollState::new(0.0)).with(|state| state.clone());
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(8.0)
            .clip_to_bounds()
            .horizontal_scroll(tabs_scroll_state, false),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for tab in DEMO_TABS {
                TabButton(tab, active_tab, 10.0);
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn TabContent(
    active_tab: cranpose_core::MutableState<DemoTab>,
    startup: StartupSelection,
    winamp_tab_state: WinampTabState,
    modifier: Modifier,
) {
    let active = active_tab.get();
    cranpose_ui::Box(modifier.clip_to_bounds(), BoxSpec::default(), move || {
        cranpose_core::with_key(&active, || {
            if tab_requires_scroll(active) {
                ScrollableTab(move || render_active_tab(active, startup, winamp_tab_state));
            } else {
                render_active_tab(active, startup, winamp_tab_state);
            }
        });
    });
}

#[composable]
pub fn combined_app() {
    combined_app_with_startup(StartupSelection::default());
}

#[composable]
pub fn combined_app_with_initial_tab(initial_tab: Option<DemoTab>) {
    combined_app_with_startup(StartupSelection {
        initial_tab,
        initial_shader_section: None,
    });
}

#[composable]
pub fn combined_app_with_startup(startup: StartupSelection) {
    let initial_tab = startup.initial_tab.unwrap_or(DemoTab::Counter);
    let active_tab = cranpose_core::useState(move || initial_tab);
    let winamp_tab_state = remember_winamp_tab_state();
    TEST_ACTIVE_TAB_STATE.with(|cell| {
        *cell.borrow_mut() = Some(active_tab);
    });

    Column(
        Modifier::empty().fill_max_size().padding(20.0),
        ColumnSpec::default(),
        move || {
            TabBarHorizontal(active_tab);

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            TabContent(
                active_tab,
                startup,
                winamp_tab_state,
                Modifier::empty().fill_max_width().weight(1.0),
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
pub fn MarkdownViewerRobotApp() {
    markdown_viewer_tab();
}

#[allow(non_snake_case)]
#[composable]
pub fn HackerNewsScrollStabilityRobotApp() {
    HackerNewsScrollStabilityFixtureTab();
}

#[allow(non_snake_case)]
#[composable]
pub fn MarkdownScrollStabilityRobotApp() {
    MarkdownScrollStabilityFixtureTab();
}

#[allow(non_snake_case)]
#[composable]
pub fn MarkdownScrollStressRobotApp() {
    MarkdownScrollStressFixtureTab();
}

#[allow(non_snake_case)]
#[composable]
pub fn MarkdownScrollStressRobotAppWithState(list_state: LazyListState) {
    MarkdownScrollStressFixtureTabWithState(list_state);
}

fn tab_requires_scroll(tab: DemoTab) -> bool {
    !matches!(
        tab,
        DemoTab::HackerNews
            | DemoTab::LazyList
            | DemoTab::Winamp
            | DemoTab::MarkdownViewer
            | DemoTab::Liquid
    )
}

#[composable]
fn render_active_tab(active: DemoTab, startup: StartupSelection, winamp_tab_state: WinampTabState) {
    match active {
        DemoTab::Counter => counter_app(),
        DemoTab::CompositionLocal => composition_local_example(),
        DemoTab::Async => async_runtime_example(),
        DemoTab::Animations => AnimationsTab(),
        DemoTab::InteractiveAnim => InteractiveAnimTab(),
        DemoTab::WebFetch => web_fetch_example(),
        DemoTab::TextInput => text_input_example(),
        DemoTab::Layout => recursive_layout_example(),
        DemoTab::ModifierShowcase => modifier_showcase_tab(),
        DemoTab::LazyList => lazy_list_example(),
        DemoTab::Mineswapper2 => mineswapper2::mineswapper2_tab(),
        DemoTab::HackerNews => HackerNewsTab(),
        DemoTab::Images => images_tab(),
        DemoTab::Text => TextShowcaseTab(),
        DemoTab::Winamp => WinampTab(winamp_tab_state),
        DemoTab::Xkcd => xkcd_tab(),
        DemoTab::Shaders => ShadersTab(startup.initial_shader_section),
        DemoTab::ShaderRect => ShaderRectTab(),
        DemoTab::MarkdownViewer => markdown_viewer_tab(),
        DemoTab::FilePicker => file_picker_tab(),
        DemoTab::Liquid => LiquidUiTab(),
    }
}

/// Demonstrates the native cross-platform file/folder picker.
#[composable]
fn file_picker_tab() {
    let picker = cranpose::local_file_picker().current();
    let status = cranpose_core::useState(|| "Pick a file or folder to begin.".to_string());
    let file_request = cranpose_core::useState(|| 0u32);
    let folder_request = cranpose_core::useState(|| 0u32);

    let file_key = file_request.get();
    {
        let picker = picker.clone();
        cranpose_core::LaunchedEffectAsync!(file_key, move |_scope| Box::pin(async move {
            if file_key == 0 {
                return;
            }
            status.set("Choosing a file…".to_string());
            match picker
                .pick_file(cranpose::FilePickerOptions::default().with_title("Pick a file"))
                .await
            {
                Ok(Some(entry)) => {
                    let bytes = entry.read_bytes().await.map(|data| data.len()).unwrap_or(0);
                    status.set(format!(
                        "File: {} — {bytes} bytes\n{}",
                        entry.name(),
                        entry.display_path()
                    ));
                }
                Ok(None) => status.set("File selection cancelled.".to_string()),
                Err(error) => status.set(format!("File picker error: {error}")),
            }
        }));
    }

    let folder_key = folder_request.get();
    cranpose_core::LaunchedEffectAsync!(folder_key, move |_scope| Box::pin(async move {
        if folder_key == 0 {
            return;
        }
        status.set("Choosing a folder…".to_string());
        match picker
            .pick_folder(cranpose::FilePickerOptions::default().with_title("Pick a folder"))
            .await
        {
            Ok(Some(entry)) => {
                let count = entry.list().await.map(|items| items.len()).unwrap_or(0);
                status.set(format!(
                    "Folder: {} — {count} entries\n{}",
                    entry.name(),
                    entry.display_path()
                ));
            }
            Ok(None) => status.set("Folder selection cancelled.".to_string()),
            Err(error) => status.set(format!("Folder picker error: {error}")),
        }
    }));

    Column(
        Modifier::empty().fill_max_size().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Native File Picker",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
            Text(
                "Opens the platform's native picker. On Android, iOS and the web it surfaces the system document providers (cloud, mounted WebDAV shares, …), returning an opaque handle rather than a local path.",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
            picker_button("Pick a file", move || {
                file_request.set(file_request.get() + 1)
            });
            picker_button("Pick a folder", move || {
                folder_request.set(folder_request.get() + 1)
            });
            Text(
                status.get(),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn picker_button(label: &'static str, on_click: impl FnMut() + 'static) {
    Button(
        Modifier::empty()
            .rounded_corners(12.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(0.2, 0.45, 0.9, 1.0)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(12.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(label, Modifier::empty().padding(4.0), TextStyle::default());
        },
    );
}

/// Text Input Demo Tab - showcases BasicTextField functionality
#[composable]
fn text_input_example() {
    // Create text field states using cranpose_core::remember
    let text_state1 =
        cranpose_core::remember(|| TextFieldState::new("Type here...")).with(|state| state.clone());
    let text_state2 =
        cranpose_core::remember(|| TextFieldState::new("")).with(|state| state.clone());

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.10, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Text Input Demo",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(16.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 24.0,
            });

            // First text field with label
            Text(
                "Basic Text Field:",
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            // Text field with background styling
            {
                let state = text_state1.clone();
                BasicTextField(
                    state,
                    Modifier::empty()
                        .fill_max_width()
                        .padding(12.0)
                        .background(Color(0.15, 0.18, 0.25, 1.0))
                        .rounded_corners(8.0),
                    TextStyle::default(),
                );
            }

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Show current text value - this now updates when version changes
            {
                // Reading text() creates composition dependency - scope recomposes when text changes
                let current_text = text_state1.text();
                Text(
                    format!("Current value: \"{}\"", current_text),
                    Modifier::empty()
                        .padding(8.0)
                        .background(Color(0.12, 0.16, 0.28, 0.8))
                        .rounded_corners(8.0),
                    TextStyle::default(),
                );
            }

            Spacer(Size {
                width: 0.0,
                height: 24.0,
            });

            // Second text field
            Text(
                "Empty Text Field:",
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            {
                let state = text_state2.clone();
                BasicTextField(
                    state,
                    Modifier::empty()
                        .fill_max_width()
                        .padding(12.0)
                        .background(Color(0.18, 0.15, 0.22, 1.0))
                        .rounded_corners(8.0),
                    TextStyle::default(),
                );
            }

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Buttons to manipulate text programmatically
            Text(
                "Programmatic Actions:",
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let state1 = text_state1.clone();
                    let state2 = text_state2.clone();
                    move || {
                        // Clear button
                        {
                            let state = state1.clone();
                            Button(
                                Modifier::empty()
                                    .rounded_corners(8.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.6, 0.2, 0.2, 1.0)),
                                            CornerRadii::uniform(8.0),
                                        );
                                    })
                                    .padding(10.0),
                                ButtonSpec::default(),
                                move || {
                                    state.set_text("");
                                },
                                || {
                                    Text(
                                        "Clear",
                                        Modifier::empty().padding(4.0),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        }

                        // Add text button
                        {
                            let state = state1.clone();
                            Button(
                                Modifier::empty()
                                    .rounded_corners(8.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.2, 0.5, 0.3, 1.0)),
                                            CornerRadii::uniform(8.0),
                                        );
                                    })
                                    .padding(10.0),
                                ButtonSpec::default(),
                                move || {
                                    state.edit(|buffer| {
                                        buffer.place_cursor_at_end();
                                        buffer.insert("!");
                                    });
                                    // No version.set() needed - TextFieldState triggers recomposition
                                },
                                || {
                                    Text(
                                        "Add !",
                                        Modifier::empty().padding(4.0),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        }

                        // Copy to second field
                        {
                            let from = state1.clone();
                            let to = state2.clone();
                            Button(
                                Modifier::empty()
                                    .rounded_corners(8.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.2, 0.4, 0.6, 1.0)),
                                            CornerRadii::uniform(8.0),
                                        );
                                    })
                                    .padding(10.0),
                                ButtonSpec::default(),
                                move || {
                                    let text = from.text();
                                    to.set_text(text);
                                    // No version.set() needed - TextFieldState triggers recomposition
                                },
                                || {
                                    Text(
                                        "Copy ↓",
                                        Modifier::empty().padding(4.0),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        }
                    }
                },
            );
        },
    );
}

#[composable]
fn recursive_layout_example() {
    let depth_state = cranpose_core::useState(|| 3usize);
    TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| {
        *cell.borrow_mut() = Some(depth_state);
    });

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.10, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                cached_recursive_static_text("Recursive Layout Playground"),
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(16.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Row(
                Modifier::empty().fill_max_width().padding(8.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    move || {
                        let depth = depth_state.get();
                        Button(
                            Modifier::empty()
                                .background(Color(0.35, 0.45, 0.85, 1.0))
                                .rounded_corners(16.0)
                                .padding(10.0),
                            ButtonSpec::default(),
                            {
                                move || {
                                    let next = (depth_state.get() + 1).min(96);
                                    if next != depth_state.get() {
                                        depth_state.set(next);
                                    }
                                }
                            },
                            || {
                                Text(
                                    cached_recursive_static_text("Increase depth"),
                                    Modifier::empty().padding(6.0),
                                    TextStyle::default(),
                                );
                            },
                        );

                        Button(
                            Modifier::empty()
                                .background(Color(0.65, 0.35, 0.35, 1.0))
                                .rounded_corners(16.0)
                                .padding(10.0),
                            ButtonSpec::default(),
                            {
                                move || {
                                    let next = depth_state.get().saturating_sub(1).max(1);
                                    if next != depth_state.get() {
                                        depth_state.set(next);
                                    }
                                }
                            },
                            || {
                                Text(
                                    cached_recursive_static_text("Decrease depth"),
                                    Modifier::empty().padding(6.0),
                                    TextStyle::default(),
                                );
                            },
                        );

                        Text(
                            cached_current_depth_text(depth.max(1)),
                            Modifier::empty()
                                .padding(8.0)
                                .background(Color(0.12, 0.16, 0.28, 0.8))
                                .rounded_corners(12.0),
                            TextStyle::default(),
                        );
                    }
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            let depth = depth_state.get().max(1);
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .semantics(|config: &mut SemanticsConfiguration| {
                        config.content_description = Some("RecursiveLayoutViewport".to_string());
                    })
                    .padding(8.0)
                    .background(Color(0.06, 0.08, 0.16, 0.9))
                    .rounded_corners(20.0)
                    .padding(12.0),
                ColumnSpec::default(),
                move || {
                    recursive_layout_node(Modifier::empty(), depth, true, 0);
                },
            );
        },
    );
}

#[composable]
fn recursive_layout_node(modifier: Modifier, depth: usize, horizontal: bool, index: usize) {
    let accent = [
        Color(0.25, 0.32, 0.58, 0.75),
        Color(0.30, 0.20, 0.45, 0.75),
        Color(0.20, 0.40, 0.32, 0.75),
        Color(0.45, 0.28, 0.24, 0.75),
    ][index % 4];

    Column(
        modifier
            .background(accent)
            .rounded_corners(18.0)
            .padding(12.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                cached_depth_text(depth),
                Modifier::empty()
                    .padding(6.0)
                    .background(Color(0.0, 0.0, 0.0, 0.25))
                    .rounded_corners(10.0),
                TextStyle::default(),
            );

            if depth <= 1 {
                Text(
                    cached_recursive_static_text("Leaf node"),
                    Modifier::empty()
                        .padding(6.0)
                        .background(Color(1.0, 1.0, 1.0, 0.12))
                        .rounded_corners(10.0),
                    TextStyle::default(),
                );
            } else if horizontal {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(
                                Modifier::empty().rowWeight(1.0, true),
                                depth - 1,
                                false,
                                index * 2 + child_idx + 1,
                            );
                        }
                    },
                );
            } else {
                Column(
                    Modifier::empty().fill_max_width(),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(
                                Modifier::empty().columnWeight(1.0, true),
                                depth - 1,
                                true,
                                index * 2 + child_idx + 1,
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
pub fn composition_local_example() {
    let counter = cranpose_core::useState(|| 0);

    TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| {
        *cell.borrow_mut() = Some(counter);
    });

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.12, 0.10, 0.24, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "CompositionLocal Subscription Test",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.1))
                    .rounded_corners(16.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.2, 0.3, 0.4, 0.7))
                    .rounded_corners(12.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Button(
                Modifier::empty()
                    .rounded_corners(16.0)
                    .draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.2, 0.45, 0.9, 1.0)),
                            CornerRadii::uniform(16.0),
                        );
                    })
                    .padding(12.0),
                ButtonSpec::default(),
                {
                    move || {
                        let new_val = counter.get() + 1;
                        counter.set(new_val);
                    }
                },
                || {
                    Text(
                        "Increment",
                        Modifier::empty().padding(6.0),
                        TextStyle::default(),
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            let local = local_holder();
            let count = counter.get();

            CompositionLocalProvider(vec![local.provides(Holder { count })], || {
                composition_local_content();
            });
        },
    );
}

#[composable]
fn composition_local_content() {
    Text(
        format!("Outside provider (NOT reading): rand={}", random()),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.3, 0.3, 0.3, 0.5))
            .rounded_corners(12.0),
        TextStyle::default(),
    );

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    composition_local_content_inner();

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    Text(
        format!("NOT reading local: rand={}", random()),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.9, 0.6, 0.4, 0.5))
            .rounded_corners(12.0),
        TextStyle::default(),
    );
}

#[composable]
fn composition_local_content_inner() {
    let local = local_holder();
    let holder = local.current();
    Text(
        format!("READING local: count={}, rand={}", holder.count, random()),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.6, 0.9, 0.4, 0.7))
            .rounded_corners(12.0),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
pub fn AsyncRuntimeTabContent(
    animation: MutableState<AnimationState>,
    stats: MutableState<FrameStats>,
    is_running: MutableState<bool>,
    reset_signal: MutableState<u64>,
) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .semantics(|config: &mut SemanticsConfiguration| {
                config.content_description = Some("AsyncRuntimeViewport".to_string());
            })
            .padding(32.0)
            .background(Color(0.10, 0.14, 0.28, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        {
            move || {
                Text(
                    "Async Runtime Demo",
                    Modifier::empty()
                        .padding(12.0)
                        .background(Color(1.0, 1.0, 1.0, 0.08))
                        .rounded_corners(16.0),
                    TextStyle::default(),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                let animation_snapshot = animation.get();
                let stats_snapshot = stats.get();
                let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);
                Column(
                    Modifier::empty()
                        .fill_max_width()
                        .padding(8.0)
                        .background(Color(0.06, 0.10, 0.22, 0.8))
                        .rounded_corners(18.0)
                        .padding(12.0),
                    ColumnSpec::default(),
                    {
                        move || {
                            Text(
                                format!("Progress: {:>3}%", (progress_value * 100.0) as i32),
                                Modifier::empty().padding(6.0),
                                TextStyle::default(),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 8.0,
                            });

                            Row(
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(26.0)
                                    .rounded_corners(13.0)
                                    .semantics(|config: &mut SemanticsConfiguration| {
                                        config.content_description =
                                            Some("AsyncProgressBarTrack".to_string());
                                    })
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.12, 0.16, 0.30, 1.0)),
                                            CornerRadii::uniform(13.0),
                                        );
                                    }),
                                RowSpec::default(),
                                move || {
                                    if progress_value > 0.0 {
                                        Row(
                                            Modifier::empty()
                                                .fill_max_width_fraction(progress_value)
                                                .height(26.0)
                                                .then(Modifier::empty().rounded_corners(13.0))
                                                .semantics(|config: &mut SemanticsConfiguration| {
                                                    config.content_description =
                                                        Some("AsyncProgressBarFill".to_string());
                                                })
                                                .draw_behind(|scope| {
                                                    scope.draw_round_rect(
                                                        Brush::linear_gradient(vec![
                                                            Color(0.25, 0.55, 0.95, 1.0),
                                                            Color(0.15, 0.35, 0.80, 1.0),
                                                        ]),
                                                        CornerRadii::uniform(13.0),
                                                    );
                                                }),
                                            RowSpec::default(),
                                            || {},
                                        );
                                    }
                                },
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                Text(
                    format!(
                        "Frames advanced: {} (last frame {:.2} ms, direction: {})",
                        stats_snapshot.frames,
                        stats_snapshot.last_frame_ms,
                        if animation_snapshot.direction >= 0.0 {
                            "forward"
                        } else {
                            "reverse"
                        }
                    ),
                    Modifier::empty()
                        .padding(8.0)
                        .background(Color(0.18, 0.22, 0.36, 0.6))
                        .rounded_corners(14.0),
                    TextStyle::default(),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Row(
                    Modifier::empty().fill_max_width().padding(4.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    move || {
                        let running = is_running.get();
                        let button_color = if running {
                            Color(0.50, 0.20, 0.35, 1.0)
                        } else {
                            Color(0.2, 0.45, 0.9, 1.0)
                        };
                        Button(
                            Modifier::empty()
                                .rounded_corners(16.0)
                                .draw_behind(move |scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(button_color),
                                        CornerRadii::uniform(16.0),
                                    );
                                })
                                .padding(12.0),
                            ButtonSpec::default(),
                            move || {
                                is_running.set(!is_running.get());
                            },
                            {
                                let label = if running {
                                    "Pause animation"
                                } else {
                                    "Resume animation"
                                };
                                move || {
                                    Text(
                                        label,
                                        Modifier::empty().padding(6.0),
                                        TextStyle::default(),
                                    );
                                }
                            },
                        );

                        Button(
                            Modifier::empty()
                                .rounded_corners(16.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.16, 0.36, 0.82, 1.0)),
                                        CornerRadii::uniform(16.0),
                                    );
                                })
                                .padding(12.0),
                            ButtonSpec::default(),
                            move || {
                                animation.set(AnimationState::default());
                                stats.set(FrameStats::default());
                                if !is_running.get() {
                                    is_running.set(true);
                                }
                                reset_signal.update(|tick| *tick = tick.wrapping_add(1));
                            },
                            || {
                                Text(
                                    "Reset",
                                    Modifier::empty().padding(6.0),
                                    TextStyle::default(),
                                );
                            },
                        );
                    },
                );
            }
        },
    );
}

#[composable]
#[allow(non_snake_case)]
pub(crate) fn AsyncRuntimeEngine(
    animation: MutableState<AnimationState>,
    stats: MutableState<FrameStats>,
    is_running: MutableState<bool>,
    reset_signal: MutableState<u64>,
) {
    let transition = rememberInfiniteTransition("async_runtime_engine");
    let spec = infiniteRepeatable(
        AnimationSpec::linear(1200),
        RepeatMode::Reverse,
        StartOffset::default(),
    );
    let duration_ms = spec.animation.duration_millis as f32;
    let progress_state = transition.animateFloat(0.0, 1.0, spec, "async_progress");
    let progress_value = progress_state.value();

    let last_progress = cranpose_core::useState(|| progress_value);
    let last_reset = cranpose_core::useState(|| reset_signal.get());
    let running = is_running.get();
    let reset_key = reset_signal.get();

    // Handle reset via LaunchedEffect (only triggers on reset_key change)
    cranpose_core::LaunchedEffect!((reset_key,), move |_scope| {
        if last_reset.get() != reset_key {
            last_reset.set(reset_key);
            animation.set(AnimationState::default());
            stats.set(FrameStats::default());
            last_progress.set(progress_value);
        }
    });

    // Per-frame progress tracking: computed inline during composition,
    // not in a LaunchedEffect (which would re-launch every frame and leak).
    if running {
        let previous = last_progress.get();
        let delta = progress_value - previous;
        let direction = if delta >= 0.0 { 1.0 } else { -1.0 };
        let dt_ms = (delta.abs() * duration_ms).max(0.0);

        stats.update(|state| {
            state.frames = state.frames.wrapping_add(1);
            state.last_frame_ms = dt_ms;
        });
        animation.update(|anim| {
            anim.progress = progress_value;
            anim.direction = direction;
        });
    }
    last_progress.set(progress_value);
}

#[composable]
fn async_runtime_example() {
    let animation = cranpose_core::useState(AnimationState::default);
    let stats = cranpose_core::useState(FrameStats::default);
    let is_running = cranpose_core::useState(|| true);
    let reset_signal = cranpose_core::useState(|| 0u64);

    AsyncRuntimeEngine(animation, stats, is_running, reset_signal);
    AsyncRuntimeTabContent(animation, stats, is_running, reset_signal);
}

#[composable]
fn counter_app() {
    let counter = cranpose_core::useState(|| 0);
    let pointer_position = cranpose_core::useState(|| Point { x: 0.0, y: 0.0 });
    let pointer_down = cranpose_core::useState(|| false);
    TEST_COUNTER_APP_COUNTER_STATE.with(|cell| {
        *cell.borrow_mut() = Some(counter);
    });
    TEST_COUNTER_APP_POINTER_DOWN_STATE.with(|cell| {
        *cell.borrow_mut() = Some(pointer_down);
    });
    TEST_COUNTER_APP_POINTER_POSITION_STATE.with(|cell| {
        *cell.borrow_mut() = Some(pointer_position);
    });
    let async_message =
        cranpose_core::useState(|| "Tap \"Fetch async value\" to run background work".to_string());
    let fetch_request = cranpose_core::useState(|| 0u64);
    let pointer = pointer_position.get();
    let pointer_wave = (pointer.x / 360.0).clamp(0.0, 1.0);
    let target_wave = if pointer_down.get() {
        0.6 + pointer_wave * 0.4
    } else {
        pointer_wave * 0.6
    };
    let wave_state = animateFloatAsState(target_wave, AnimationType::default(), "wave");
    let fetch_key = fetch_request.get();
    LaunchedEffect!(fetch_key, move |_scope| {
        if fetch_key == 0 {
            return;
        }
        _scope.launch_background(
            move |token| async move {
                if token.is_cancelled() {
                    return String::new();
                }
                // Simulate background work with a delay on native
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use instant::Duration;
                    use std::thread;
                    for _ in 0..5 {
                        if token.is_cancelled() {
                            return String::new();
                        }
                        thread::sleep(Duration::from_millis(80));
                    }
                }
                let val = demo_random_u32() % 1000;
                format!("Background fetch #{fetch_key}: {val}")
            },
            move |value| {
                if value.is_empty() {
                    return;
                }
                async_message.set(value);
            },
        );
    });
    let is_even = counter.get() % 2 == 0;

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        cranpose_core::with_key(&is_even, move || {
            if is_even {
                Text(
                    "if counter % 2 == 0",
                    Modifier::empty()
                        .padding(12.0)
                        .then(
                            Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                                16.0, 24.0, 16.0, 24.0,
                            )),
                        )
                        .draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                                CornerRadii::uniform(20.0),
                            );
                        }),
                    TextStyle::default(),
                );
            } else {
                Text(
                    "if counter % 2 != 0",
                    Modifier::empty()
                        .padding(12.0)
                        .then(
                            Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                                16.0, 24.0, 16.0, 24.0,
                            )),
                        )
                        .draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.5)),
                                CornerRadii::uniform(20.0),
                            );
                        }),
                    TextStyle::default(),
                );
            }
        });
    });

    cranpose_ui::Box(Modifier::empty(), BoxSpec::default(), move || {
        Column(
            Modifier::empty()
                .padding(32.0)
                .rounded_corners(24.0)
                .draw_behind(move |scope| {
                    let phase = wave_state.value();
                    scope.draw_round_rect(
                        Brush::linear_gradient(vec![
                            Color(0.12 + phase * 0.2, 0.10, 0.24 + (1.0 - phase) * 0.3, 1.0),
                            Color(0.08, 0.16 + (1.0 - phase) * 0.3, 0.26 + phase * 0.2, 1.0),
                        ]),
                        CornerRadii::uniform(24.0),
                    );
                })
                .padding(20.0),
            ColumnSpec::default(),
            move || {
                Text(
                    "Cranpose Playground",
                    Modifier::empty()
                        .padding(12.0)
                        .then(
                            Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                                16.0, 24.0, 16.0, 24.0,
                            )),
                        )
                        .draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                                CornerRadii::uniform(20.0),
                            );
                        }),
                    TextStyle::default(),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                Row(
                    Modifier::empty().fill_max_width().padding(8.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    move || {
                        Text(
                            format!("Counter: {}", counter.get()),
                            Modifier::empty()
                                .padding(8.0)
                                .then(Modifier::empty().background(Color(0.0, 0.0, 0.0, 0.35)))
                                .rounded_corners(12.0),
                            TextStyle::default(),
                        );
                        Text(
                            "Wave layer-only animation",
                            Modifier::empty()
                                .padding(8.0)
                                .then(Modifier::empty().background(Color(0.35, 0.55, 0.9, 0.5)))
                                .rounded_corners(12.0)
                                .graphics_layer(move || {
                                    let wave_value = wave_state.value();
                                    GraphicsLayer {
                                        alpha: 0.7 + wave_value * 0.3,
                                        scale: 0.85 + wave_value * 0.3,
                                        translation_y: (wave_value - 0.5) * 12.0,
                                        ..Default::default()
                                    }
                                }),
                            TextStyle::default(),
                        );
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Column(
                    Modifier::empty()
                        .rounded_corners(20.0)
                        .draw_with_cache(|cache| {
                            cache.on_draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.16, 0.18, 0.26, 0.95)),
                                    CornerRadii::uniform(20.0),
                                );
                            });
                        })
                        .draw_with_content({
                            let position = pointer_position.get();
                            let pressed = pointer_down.get();
                            move |scope| {
                                let intensity = if pressed { 0.45 } else { 0.25 };
                                scope.draw_round_rect(
                                    Brush::radial_gradient(
                                        vec![
                                            Color(0.4, 0.6, 1.0, intensity),
                                            Color(0.2, 0.3, 0.6, 0.0),
                                        ],
                                        position,
                                        120.0,
                                    ),
                                    CornerRadii::uniform(20.0),
                                );
                            }
                        })
                        .pointer_input((), {
                            move |scope: PointerInputScope| async move {
                                scope
                                    .await_pointer_event_scope(|await_scope| async move {
                                        loop {
                                            let event = await_scope.await_pointer_event().await;
                                            match event.kind {
                                                PointerEventKind::Down => pointer_down.set(true),
                                                PointerEventKind::Up => pointer_down.set(false),
                                                PointerEventKind::Move => {
                                                    pointer_position.set(Point {
                                                        x: event.position.x,
                                                        y: event.position.y,
                                                    });
                                                }
                                                PointerEventKind::Cancel => pointer_down.set(false),
                                                PointerEventKind::Scroll
                                                | PointerEventKind::Zoom
                                                | PointerEventKind::Enter
                                                | PointerEventKind::Exit => {}
                                            }
                                        }
                                    })
                                    .await;
                            }
                        })
                        .padding(16.0),
                    ColumnSpec::default(),
                    move || {
                        Text(
                            format!("Pointer: ({:.1}, {:.1})", pointer.x, pointer.y),
                            Modifier::empty()
                                .padding(8.0)
                                .background(Color(0.1, 0.1, 0.15, 0.6))
                                .rounded_corners(12.0)
                                .padding(8.0),
                            TextStyle::default(),
                        );

                        Spacer(Size {
                            width: 0.0,
                            height: 16.0,
                        });

                        Row(
                            Modifier::empty()
                                .padding(8.0)
                                .rounded_corners(12.0)
                                .background(Color(0.1, 0.1, 0.15, 0.6))
                                .padding(8.0),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                                .vertical_alignment(VerticalAlignment::CenterVertically),
                            || {
                                Button(
                                    Modifier::empty()
                                        .width_intrinsic(IntrinsicSize::Max)
                                        .rounded_corners(12.0)
                                        .draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(Color(0.3, 0.5, 0.2, 1.0)),
                                                CornerRadii::uniform(12.0),
                                            );
                                        })
                                        .padding(10.0),
                                    ButtonSpec::default(),
                                    || {},
                                    || {
                                        Text(
                                            "OK",
                                            Modifier::empty().padding(4.0).then(
                                                Modifier::empty().size(Size {
                                                    width: 50.0,
                                                    height: 50.0,
                                                }),
                                            ),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                                Button(
                                    Modifier::empty()
                                        .width_intrinsic(IntrinsicSize::Max)
                                        .rounded_corners(12.0)
                                        .draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(Color(0.5, 0.3, 0.2, 1.0)),
                                                CornerRadii::uniform(12.0),
                                            );
                                        })
                                        .padding(10.0),
                                    ButtonSpec::default(),
                                    || {},
                                    || {
                                        Text(
                                            "Cancel",
                                            Modifier::empty().padding(4.0),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                                Button(
                                    Modifier::empty()
                                        .width_intrinsic(IntrinsicSize::Max)
                                        .rounded_corners(12.0)
                                        .draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(Color(0.2, 0.3, 0.5, 1.0)),
                                                CornerRadii::uniform(12.0),
                                            );
                                        })
                                        .padding(10.0),
                                    ButtonSpec::default(),
                                    || {},
                                    || {
                                        Text(
                                            "Long Button Text",
                                            Modifier::empty().padding(4.0),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                            },
                        );

                        Spacer(Size {
                            width: 0.0,
                            height: 16.0,
                        });

                        Row(
                            Modifier::empty().padding(8.0),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                            move || {
                                Button(
                                    Modifier::empty()
                                        .rounded_corners(16.0)
                                        .draw_with_cache(|cache| {
                                            cache.on_draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::linear_gradient(vec![
                                                        Color(0.2, 0.45, 0.9, 1.0),
                                                        Color(0.15, 0.3, 0.65, 1.0),
                                                    ]),
                                                    CornerRadii::uniform(16.0),
                                                );
                                            });
                                        })
                                        .padding(12.0),
                                    ButtonSpec::default(),
                                    move || {
                                        println!("Incrementing counter to {}", counter.get() + 1);
                                        counter.set(counter.get() + 1)
                                    },
                                    || {
                                        Text(
                                            "Increment",
                                            Modifier::empty().padding(6.0),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                                Button(
                                    Modifier::empty()
                                        .rounded_corners(16.0)
                                        .draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(Color(0.4, 0.18, 0.3, 1.0)),
                                                CornerRadii::uniform(16.0),
                                            );
                                        })
                                        .padding(12.0),
                                    ButtonSpec::default(),
                                    move || counter.set(counter.get() - 1),
                                    || {
                                        Text(
                                            "Decrement",
                                            Modifier::empty().padding(6.0),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                            },
                        );

                        Spacer(Size {
                            width: 0.0,
                            height: 20.0,
                        });

                        Text(
                            async_message.get(),
                            Modifier::empty()
                                .padding(10.0)
                                .background(Color(0.1, 0.18, 0.32, 0.6))
                                .rounded_corners(14.0),
                            TextStyle::default(),
                        );

                        Spacer(Size {
                            width: 0.0,
                            height: 12.0,
                        });

                        Button(
                            Modifier::empty()
                                .rounded_corners(16.0)
                                .draw_with_cache(|cache| {
                                    cache.on_draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::linear_gradient(vec![
                                                Color(0.15, 0.35, 0.85, 1.0),
                                                Color(0.08, 0.2, 0.55, 1.0),
                                            ]),
                                            CornerRadii::uniform(16.0),
                                        );
                                    });
                                })
                                .padding(12.0),
                            ButtonSpec::default(),
                            {
                                move || {
                                    async_message
                                        .set("Fetching value on background thread...".to_string());
                                    fetch_request.update(|value| *value += 1);
                                }
                            },
                            || {
                                Text(
                                    "Fetch async value",
                                    Modifier::empty().padding(6.0),
                                    TextStyle::default(),
                                );
                            },
                        );
                    },
                );
            },
        );
    });
}

#[composable]
fn composition_local_observer() {
    let state = cranpose_core::useState(|| 0);
    DisposableEffect!((), move |_| {
        state.set(state.get() + 1);
        DisposableEffectResult::default()
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ShowcaseType {
    SimpleCard,
    PositionedBoxes,
    ItemList,
    ComplexChain,
    DynamicModifiers,
    LongList,
}

impl ShowcaseType {
    fn label(self) -> &'static str {
        match self {
            ShowcaseType::SimpleCard => "Simple Card",
            ShowcaseType::PositionedBoxes => "Positioned Boxes",
            ShowcaseType::ItemList => "Item List (5)",
            ShowcaseType::ComplexChain => "Complex Chain",
            ShowcaseType::DynamicModifiers => "Dynamic Modifiers",
            ShowcaseType::LongList => "Long List (50)",
        }
    }
}

#[composable]
fn modifier_showcase_tab() {
    let selected_showcase = cranpose_core::useState(|| ShowcaseType::SimpleCard);

    Row(
        Modifier::empty().fill_max_width().padding(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::Top),
        move || {
            // Left panel - showcase selector
            Column(
                Modifier::empty()
                    .width(180.0)
                    .padding(16.0)
                    .background(Color(0.08, 0.10, 0.18, 1.0))
                    .rounded_corners(20.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(
                        "Select Showcase",
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(1.0, 1.0, 1.0, 0.08))
                            .rounded_corners(12.0),
                        TextStyle::default(),
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 8.0,
                    });

                    let showcase_types = [
                        ShowcaseType::SimpleCard,
                        ShowcaseType::PositionedBoxes,
                        ShowcaseType::ItemList,
                        ShowcaseType::ComplexChain,
                        ShowcaseType::DynamicModifiers,
                        ShowcaseType::LongList,
                    ];

                    for showcase_type in showcase_types {
                        let is_selected = selected_showcase.get() == showcase_type;
                        Button(
                            Modifier::empty()
                                .fill_max_width()
                                .rounded_corners(10.0)
                                .draw_behind(move |scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(if is_selected {
                                            Color(0.25, 0.45, 0.85, 1.0)
                                        } else {
                                            Color(0.15, 0.18, 0.25, 0.8)
                                        }),
                                        CornerRadii::uniform(10.0),
                                    );
                                })
                                .padding(10.0),
                            ButtonSpec::default(),
                            {
                                move || {
                                    if selected_showcase.get() != showcase_type {
                                        selected_showcase.set(showcase_type);
                                    }
                                }
                            },
                            {
                                let label = showcase_type.label();
                                move || {
                                    Text(
                                        label,
                                        Modifier::empty().padding(4.0),
                                        TextStyle::default(),
                                    );
                                }
                            },
                        );
                    }
                },
            );

            // Right panel - showcase content
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .padding(24.0)
                    .background(Color(0.06, 0.08, 0.16, 0.9))
                    .rounded_corners(20.0)
                    .padding(16.0),
                ColumnSpec::default(),
                move || {
                    let showcase_to_render = selected_showcase.get();
                    cranpose_core::with_key(&showcase_to_render, || match showcase_to_render {
                        ShowcaseType::SimpleCard => simple_card_showcase(),
                        ShowcaseType::PositionedBoxes => positioned_boxes_showcase(),
                        ShowcaseType::ItemList => item_list_showcase(),
                        ShowcaseType::ComplexChain => complex_chain_showcase(),
                        ShowcaseType::DynamicModifiers => dynamic_modifiers_showcase(),
                        ShowcaseType::LongList => long_list_showcase(),
                    });
                },
            );
        },
    );
}

#[composable]
pub fn simple_card_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Simple Card Pattern ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        // Card with border effect (outer box creates border)
        cranpose_ui::Box(
            Modifier::empty()
                .padding(3.0)
                .background(Color(0.4, 0.6, 0.9, 0.8))
                .rounded_corners(18.0),
            BoxSpec::default(),
            || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .padding(16.0)
                        .size(Size {
                            width: 300.0,
                            height: 200.0,
                        })
                        .background(Color(0.15, 0.18, 0.25, 0.95))
                        .rounded_corners(16.0),
                    BoxSpec::default(),
                    || {
                        Column(
                            Modifier::empty().padding(8.0),
                            ColumnSpec::default(),
                            || {
                                Text(
                                    "Card Title",
                                    Modifier::empty()
                                        .padding(8.0)
                                        .background(Color(0.3, 0.5, 0.8, 0.6))
                                        .rounded_corners(8.0),
                                    TextStyle::default(),
                                );

                                Spacer(Size {
                                    width: 0.0,
                                    height: 8.0,
                                });

                                Text(
                                    "Card content goes here with padding",
                                    Modifier::empty().padding(4.0),
                                    TextStyle::default(),
                                );

                                Spacer(Size {
                                    width: 0.0,
                                    height: 12.0,
                                });

                                // Action buttons row
                                Row(Modifier::empty(), RowSpec::default(), || {
                                    Text(
                                        "Action 1",
                                        Modifier::empty()
                                            .padding(8.0)
                                            .background(Color(0.2, 0.7, 0.4, 0.7))
                                            .rounded_corners(6.0),
                                        TextStyle::default(),
                                    );

                                    Spacer(Size {
                                        width: 8.0,
                                        height: 0.0,
                                    });

                                    Text(
                                        "Action 2",
                                        Modifier::empty()
                                            .padding(8.0)
                                            .background(Color(0.8, 0.3, 0.3, 0.7))
                                            .rounded_corners(6.0),
                                        TextStyle::default(),
                                    );
                                });
                            },
                        );
                    },
                );
            },
        );
    });
}

#[composable]
pub fn positioned_boxes_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Positioned Boxes ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        // Wrap positioned boxes in a container with explicit size
        // This allows overlapping boxes with offset positioning
        cranpose_ui::Box(
            Modifier::empty()
                .size_points(360.0, 280.0)
                .background(Color(0.05, 0.05, 0.15, 0.5))
                .rounded_corners(8.0),
            BoxSpec::default(),
            || {
                // Box A - Purple, top-left
                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(100.0, 100.0)
                        .offset(20.0, 20.0)
                        .padding(8.0)
                        .background(Color(0.6, 0.2, 0.7, 0.85))
                        .rounded_corners(12.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "Box A",
                            Modifier::empty().padding(6.0),
                            TextStyle::default(),
                        );
                    },
                );

                // Box B - Green, bottom-right
                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(100.0, 100.0)
                        .offset(220.0, 160.0)
                        .padding(8.0)
                        .background(Color(0.2, 0.7, 0.4, 0.85))
                        .rounded_corners(12.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "Box B",
                            Modifier::empty().padding(6.0),
                            TextStyle::default(),
                        );
                    },
                );

                // Box C - Orange, center-top (smaller)
                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(80.0, 60.0)
                        .offset(140.0, 30.0)
                        .padding(6.0)
                        .background(Color(0.9, 0.5, 0.2, 0.85))
                        .rounded_corners(10.0),
                    BoxSpec::default(),
                    || {
                        Text("C", Modifier::empty().padding(4.0), TextStyle::default());
                    },
                );

                // Box D - Blue, center-left (larger)
                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(120.0, 80.0)
                        .offset(40.0, 140.0)
                        .padding(8.0)
                        .background(Color(0.2, 0.5, 0.9, 0.85))
                        .rounded_corners(14.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "Box D",
                            Modifier::empty().padding(6.0),
                            TextStyle::default(),
                        );
                    },
                );
            },
        );
    });
}

#[composable]
pub fn item_list_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Item List (5 items) ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        // List with alternating colors and borders
        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            || {
                for i in 0..5 {
                    // Alternate colors: even = blue-ish, odd = purple-ish
                    let (bg_color, border_color) = if i % 2 == 0 {
                        (Color(0.12, 0.16, 0.28, 0.8), Color(0.3, 0.5, 0.8, 0.9))
                    } else {
                        (Color(0.18, 0.12, 0.28, 0.8), Color(0.5, 0.3, 0.8, 0.9))
                    };

                    // Border wrapper
                    cranpose_ui::Box(
                        Modifier::empty()
                            .padding(2.0)
                            .background(border_color)
                            .rounded_corners(12.0),
                        BoxSpec::default(),
                        move || {
                            Row(
                                Modifier::empty()
                                    .padding(8.0)
                                    .size_points(400.0, 50.0)
                                    .background(bg_color)
                                    .rounded_corners(10.0),
                                RowSpec::default(),
                                move || {
                                    let text = match i {
                                        0 => "Item #0",
                                        1 => "Item #1",
                                        2 => "Item #2",
                                        3 => "Item #3",
                                        4 => "Item #4",
                                        _ => "Item",
                                    };
                                    Text(
                                        text,
                                        Modifier::empty().padding_horizontal(12.0),
                                        TextStyle::default(),
                                    );

                                    Spacer(Size {
                                        width: 0.0,
                                        height: 0.0,
                                    });

                                    // Status indicator
                                    let status_color = if i % 3 == 0 {
                                        Color(0.2, 0.8, 0.3, 0.9) // Green
                                    } else if i % 3 == 1 {
                                        Color(0.9, 0.7, 0.2, 0.9) // Yellow
                                    } else {
                                        Color(0.8, 0.3, 0.2, 0.9) // Red
                                    };

                                    cranpose_ui::Box(
                                        Modifier::empty()
                                            .size_points(12.0, 12.0)
                                            .background(status_color)
                                            .rounded_corners(6.0),
                                        BoxSpec::default(),
                                        || {},
                                    );
                                },
                            );
                        },
                    );
                }
            },
        );
    });
}

#[composable]
pub fn complex_chain_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Complex Modifier Chain ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            "Nested: Red → Green → Blue layers",
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        // Nested backgrounds showcase - creates visible colored borders
        // Red outer layer
        cranpose_ui::Box(
            Modifier::empty()
                .padding(8.0)
                .background(Color(0.8, 0.2, 0.2, 0.9))
                .rounded_corners(16.0),
            BoxSpec::default(),
            || {
                // Green middle layer
                cranpose_ui::Box(
                    Modifier::empty()
                        .padding(6.0)
                        .background(Color(0.2, 0.7, 0.3, 0.9))
                        .rounded_corners(12.0),
                    BoxSpec::default(),
                    || {
                        // Blue inner layer
                        cranpose_ui::Box(
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.3, 0.5, 0.9, 0.9))
                                .rounded_corners(8.0),
                            BoxSpec::default(),
                            || {
                                Text("Nested!", Modifier::empty(), TextStyle::default());
                            },
                        );
                    },
                );
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            "Chain: offset + size + multiple backgrounds",
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        // Complex modifier chain with offset and sizing - Orange outer, Purple inner
        cranpose_ui::Box(
            Modifier::empty()
                .offset(20.0, 0.0)
                .size_points(180.0, 80.0)
                .padding(6.0)
                .background(Color(0.9, 0.6, 0.2, 0.9))
                .rounded_corners(10.0),
            BoxSpec::default(),
            || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .padding(8.0)
                        .background(Color(0.5, 0.3, 0.7, 0.9))
                        .rounded_corners(6.0),
                    BoxSpec::default(),
                    || {
                        Text("Offset + Sized", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    });
}

#[composable]
pub fn dynamic_modifiers_showcase() {
    let frame = cranpose_core::useState(|| 0i32);

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            "=== Dynamic Modifiers ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        let current_frame = frame.get();
        let x = (current_frame as f32 * 10.0) % 200.0;
        let y = 50.0;

        // Wrap moving box in a container with explicit size to prevent overflow
        cranpose_ui::Box(
            Modifier::empty()
                .size_points(250.0, 150.0)
                .background(Color(0.05, 0.05, 0.15, 0.5))
                .rounded_corners(8.0),
            BoxSpec::default(),
            move || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(Size {
                            width: 50.0,
                            height: 50.0,
                        })
                        .offset(x, y)
                        .padding(6.0)
                        .background(Color(0.3, 0.6, 0.9, 0.9))
                        .rounded_corners(10.0),
                    BoxSpec::default(),
                    || {
                        Text("Move", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            format!("Frame: {}, X: {:.1}", current_frame, x),
            Modifier::empty()
                .padding(8.0)
                .background(Color(0.2, 0.2, 0.3, 0.6))
                .rounded_corners(10.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        Button(
            Modifier::empty()
                .rounded_corners(12.0)
                .draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.25, 0.45, 0.85, 1.0)),
                        CornerRadii::uniform(12.0),
                    );
                })
                .padding(10.0),
            ButtonSpec::default(),
            move || {
                frame.set(frame.get() + 1);
            },
            || {
                Text(
                    "Advance Frame",
                    Modifier::empty().padding(6.0),
                    TextStyle::default(),
                );
            },
        );
    });
}

#[composable]
pub fn long_list_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Long List (50 items) ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
            || {
                for i in 0..50 {
                    Row(
                        Modifier::empty()
                            .padding_symmetric(8.0, 4.0)
                            .size(Size {
                                width: 400.0,
                                height: 40.0,
                            })
                            .background(Color(0.12 + (i as f32 * 0.005), 0.15, 0.25, 0.7))
                            .rounded_corners(8.0),
                        RowSpec::default(),
                        move || {
                            let text = if i < 10 {
                                match i {
                                    0 => "Item 0",
                                    1 => "Item 1",
                                    2 => "Item 2",
                                    3 => "Item 3",
                                    4 => "Item 4",
                                    5 => "Item 5",
                                    6 => "Item 6",
                                    7 => "Item 7",
                                    8 => "Item 8",
                                    9 => "Item 9",
                                    _ => "Item",
                                }
                            } else {
                                "Item 10+"
                            };
                            Text(
                                text,
                                Modifier::empty().padding_horizontal(12.0),
                                TextStyle::default(),
                            );
                        },
                    );
                }
            },
        );
    });
}

#[cfg(test)]
#[path = "tests/main_tests.rs"]
mod tests;
