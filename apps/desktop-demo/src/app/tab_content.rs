// Which composable each demo tab shows, and whether it scrolls.

use cranpose_core::MutableState;
use cranpose_ui::{
    composable, BoxSpec, Column, ColumnSpec, LinearArrangement, Modifier, ScrollState,
};

use super::{
    animations::AnimationsTab, async_runtime_example, composition_local_example, counter_app,
    documentation, file_picker_tab, floating_windows::FloatingWindowsTab, gauntlet_tab,
    glass_feed::GlassFeedTab, glass_tiles::GlassTilesTab, images::images_tab,
    interactive_anim::InteractiveAnimTab, lazy_list_example, liquid_ui::LiquidUiTab,
    markdown_viewer_tab, mineswapper2, modifier_showcase_tab, performance_dashboard,
    recomposition_lab::RecompositionLabTab, recursive_layout_example, room, rotary::rotary_tab,
    shader_rect::ShaderRectTab, shaders::ShadersTab, source_view::SourcePanel, text_input_example,
    text_showcase::TextShowcaseTab, wear, web_fetch::web_fetch_example, webview, xkcd::xkcd_tab,
    ControlsUiTab, DemoTab, HackerNewsTab, StartupSelection, WszTab, WszTabState, DEMO_TAB_INFO,
};

#[composable]
pub(super) fn TabContent(
    active_tab: MutableState<DemoTab>,
    startup: StartupSelection,
    wsz_tab_state: WszTabState,
    showing_source: MutableState<bool>,
    modifier: Modifier,
) {
    let active = active_tab.get();
    let title = DEMO_TAB_INFO
        .iter()
        .find(|info| info.tab == active)
        .map_or("Demo", |info| info.label);
    cranpose_ui::Box(
        modifier.clip_to_bounds().pane_title(title),
        BoxSpec::default(),
        move || {
            if showing_source.get() {
                SourcePanel(active);
            } else {
                cranpose_core::with_key(&active, || {
                    if tab_requires_scroll(active) {
                        ScrollableTab(move || {
                            render_active_tab(active, startup, wsz_tab_state);
                        });
                    } else {
                        render_active_tab(active, startup, wsz_tab_state);
                    }
                });
            }
        },
    );
}

#[composable]
pub(crate) fn ScrollableTab(content: impl FnMut() + 'static) {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|s| *s);
    let modifier = Modifier::empty()
        .fill_max_size()
        .vertical_scroll(scroll_state, false);
    Column(
        modifier,
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(16.0)),
        content,
    );
}

pub(super) fn tab_requires_scroll(tab: DemoTab) -> bool {
    !matches!(
        tab,
        DemoTab::HackerNews
            | DemoTab::Performance
            | DemoTab::Gauntlet
            | DemoTab::Animations
            | DemoTab::RecompositionLab
            | DemoTab::WebView
            | DemoTab::Guide
            | DemoTab::LazyList
            | DemoTab::Wsz
            | DemoTab::MarkdownViewer
            | DemoTab::Liquid
            | DemoTab::GlassFeed
            | DemoTab::GlassTiles
            | DemoTab::Room
            | DemoTab::Tv
    )
}

#[composable]
pub(super) fn render_active_tab(
    active: DemoTab,
    startup: StartupSelection,
    wsz_tab_state: WszTabState,
) {
    match active {
        DemoTab::Guide => documentation::GuideTab(None),
        DemoTab::Counter => counter_app(),
        DemoTab::CompositionLocal => composition_local_example(),
        DemoTab::Async => async_runtime_example(),
        DemoTab::Animations => AnimationsTab(),
        DemoTab::InteractiveAnim => InteractiveAnimTab(),
        DemoTab::WebFetch => web_fetch_example(),
        DemoTab::WebView => webview::WebViewTab(),
        DemoTab::TextInput => text_input_example(),
        DemoTab::Layout => recursive_layout_example(),
        DemoTab::ModifierShowcase => modifier_showcase_tab(),
        DemoTab::LazyList => lazy_list_example(),
        DemoTab::Mineswapper2 => mineswapper2::mineswapper2_tab(),
        DemoTab::RecompositionLab => RecompositionLabTab(),
        DemoTab::FloatingWindows => FloatingWindowsTab(),
        DemoTab::FilePicker => file_picker_tab(),
        DemoTab::Rotary => rotary_tab(),
        DemoTab::Wear => wear::wear_tab(),
        DemoTab::HackerNews
        | DemoTab::Performance
        | DemoTab::Gauntlet
        | DemoTab::Images
        | DemoTab::Text
        | DemoTab::Wsz
        | DemoTab::Xkcd
        | DemoTab::Shaders
        | DemoTab::ShaderRect
        | DemoTab::Controls
        | DemoTab::MarkdownViewer
        | DemoTab::Liquid
        | DemoTab::GlassFeed
        | DemoTab::GlassTiles
        | DemoTab::Room
        | DemoTab::Tv => render_showcase_tab(active, startup, wsz_tab_state),
    }
}

#[composable]
fn render_showcase_tab(active: DemoTab, startup: StartupSelection, wsz_tab_state: WszTabState) {
    match active {
        DemoTab::HackerNews => HackerNewsTab(),
        DemoTab::Performance => performance_dashboard::PerformanceTab(),
        DemoTab::Gauntlet => gauntlet_tab::GauntletTab(),
        DemoTab::Images => images_tab(),
        DemoTab::Text => TextShowcaseTab(),
        DemoTab::Wsz => WszTab(wsz_tab_state),
        DemoTab::Xkcd => xkcd_tab(),
        DemoTab::Shaders => ShadersTab(startup.initial_shader_section),
        DemoTab::ShaderRect => ShaderRectTab(),
        DemoTab::Controls => ControlsUiTab(),
        DemoTab::MarkdownViewer => markdown_viewer_tab(),
        DemoTab::Liquid => LiquidUiTab(),
        DemoTab::GlassFeed => GlassFeedTab(),
        DemoTab::GlassTiles => GlassTilesTab(),
        DemoTab::Room => room::RoomTab(),
        DemoTab::Tv => super::tv::TvTab(),
        DemoTab::Guide
        | DemoTab::Counter
        | DemoTab::CompositionLocal
        | DemoTab::Async
        | DemoTab::Animations
        | DemoTab::InteractiveAnim
        | DemoTab::WebFetch
        | DemoTab::WebView
        | DemoTab::TextInput
        | DemoTab::Layout
        | DemoTab::ModifierShowcase
        | DemoTab::LazyList
        | DemoTab::Mineswapper2
        | DemoTab::RecompositionLab
        | DemoTab::FloatingWindows
        | DemoTab::FilePicker
        | DemoTab::Rotary
        | DemoTab::Wear => {}
    }
}
