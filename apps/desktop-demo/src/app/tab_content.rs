// Which composable each demo tab shows, and whether it scrolls.

use cranpose_ui::composable;

use super::{
    animations::AnimationsTab, async_runtime_example, composition_local_example, counter_app,
    documentation, file_picker_tab, floating_windows::FloatingWindowsTab, gauntlet_tab,
    glass_feed::GlassFeedTab, glass_tiles::GlassTilesTab, images::images_tab,
    interactive_anim::InteractiveAnimTab, lazy_list_example, liquid_ui::LiquidUiTab,
    markdown_viewer_tab, mineswapper2, modifier_showcase_tab, performance_dashboard,
    recomposition_lab::RecompositionLabTab, recursive_layout_example, room, rotary::rotary_tab,
    shader_rect::ShaderRectTab, shaders::ShadersTab, text_input_example,
    text_showcase::TextShowcaseTab, wear, web_fetch::web_fetch_example, webview, xkcd::xkcd_tab,
    ControlsUiTab, DemoTab, HackerNewsTab, StartupSelection, WszTab, WszTabState,
};

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
