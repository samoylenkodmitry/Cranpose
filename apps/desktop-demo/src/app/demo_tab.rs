// The demo's tabs: their labels, slugs, source files and startup names,
// and the order the tab bar shows them in.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DemoTab {
    Guide,
    Counter,
    CompositionLocal,
    Async,
    Animations,
    InteractiveAnim,
    WebFetch,
    WebView,
    TextInput,
    Layout,
    ModifierShowcase,
    LazyList,
    Mineswapper2,
    RecompositionLab,
    HackerNews,
    Performance,
    Gauntlet,
    Images,
    Text,
    Wsz,
    Xkcd,
    Shaders,
    ShaderRect,
    Controls,
    Liquid,
    GlassFeed,
    GlassTiles,
    MarkdownViewer,
    FloatingWindows,
    FilePicker,
    Rotary,
    Wear,
    Room,
}

pub const DEFAULT_INITIAL_TAB: DemoTab = DemoTab::Guide;

pub struct DemoTabInfo {
    pub tab: DemoTab,
    pub label: &'static str,
    pub slug: &'static str,
    pub source_path: &'static str,
    pub startup_aliases: &'static [&'static str],
}

pub const DEMO_TAB_INFO: [DemoTabInfo; 33] = [
    DemoTabInfo {
        tab: DemoTab::Guide,
        label: "Cranpose Guide",
        slug: "guide",
        source_path: "apps/desktop-demo/src/app/documentation.rs",
        startup_aliases: &["cranposeguide"],
    },
    DemoTabInfo {
        tab: DemoTab::Counter,
        label: "Counter App",
        slug: "counter",
        source_path: "apps/desktop-demo/src/app.rs",
        startup_aliases: &["counterapp"],
    },
    DemoTabInfo {
        tab: DemoTab::CompositionLocal,
        label: "CompositionLocal Test",
        slug: "composition-local",
        source_path: "apps/desktop-demo/src/app.rs",
        startup_aliases: &["compositionlocaltest"],
    },
    DemoTabInfo {
        tab: DemoTab::Async,
        label: "Async Runtime",
        slug: "async",
        source_path: "apps/desktop-demo/src/app.rs",
        startup_aliases: &["asyncruntime"],
    },
    DemoTabInfo {
        tab: DemoTab::Animations,
        label: "Animations",
        slug: "animations",
        source_path: "apps/desktop-demo/src/app/animations.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::InteractiveAnim,
        label: "Interactive Anim",
        slug: "interactive-anim",
        source_path: "apps/desktop-demo/src/app/interactive_anim.rs",
        startup_aliases: &["interactiveanimation", "interactiveanimations"],
    },
    DemoTabInfo {
        tab: DemoTab::WebFetch,
        label: "Web Fetch",
        slug: "web-fetch",
        source_path: "apps/desktop-demo/src/app/web_fetch.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::WebView,
        label: "WebView",
        slug: "webview",
        source_path: "apps/desktop-demo/src/app/webview.rs",
        startup_aliases: &["browser"],
    },
    DemoTabInfo {
        tab: DemoTab::TextInput,
        label: "Text Input",
        slug: "text-input",
        source_path: "apps/desktop-demo/src/app.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Layout,
        label: "Recursive Layout",
        slug: "layout",
        source_path: "apps/desktop-demo/src/app.rs",
        startup_aliases: &["recursivelayout"],
    },
    DemoTabInfo {
        tab: DemoTab::ModifierShowcase,
        label: "Modifiers Showcase",
        slug: "modifier-showcase",
        source_path: "apps/desktop-demo/src/app.rs",
        startup_aliases: &["modifiers", "modifiersshowcase"],
    },
    DemoTabInfo {
        tab: DemoTab::LazyList,
        label: "Lazy List",
        slug: "lazy-list",
        source_path: "apps/desktop-demo/src/app/lazy_list.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Mineswapper2,
        label: "Mineswapper2",
        slug: "mineswapper2",
        source_path: "apps/desktop-demo/src/app/mineswapper2.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::RecompositionLab,
        label: "Recomposition Lab",
        slug: "recomposition-lab",
        source_path: "apps/desktop-demo/src/app/recomposition_lab.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::HackerNews,
        label: "Hacker News",
        slug: "hacker-news",
        source_path: "apps/desktop-demo/src/app/hacker_news.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Performance,
        label: "Performance",
        slug: "performance",
        source_path: "apps/desktop-demo/src/app/performance_dashboard.rs",
        startup_aliases: &["perf"],
    },
    DemoTabInfo {
        tab: DemoTab::Gauntlet,
        label: "Gauntlet",
        slug: "gauntlet",
        source_path: "apps/desktop-demo/src/app/gauntlet_tab.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Images,
        label: "Images",
        slug: "images",
        source_path: "apps/desktop-demo/src/app/images.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Text,
        label: "Text",
        slug: "text",
        source_path: "apps/desktop-demo/src/app/text_showcase.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Wsz,
        label: "WSZ",
        slug: "wsz",
        source_path: "apps/desktop-demo/src/app/wsz/mod.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Xkcd,
        label: "XKCD",
        slug: "xkcd",
        source_path: "apps/desktop-demo/src/app/xkcd.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Shaders,
        label: "Shaders",
        slug: "shaders",
        source_path: "apps/desktop-demo/src/app/shaders.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::ShaderRect,
        label: "Shader Rect",
        slug: "shader-rect",
        source_path: "apps/desktop-demo/src/app/shader_rect.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Controls,
        label: "Controls UI",
        slug: "controls-ui",
        source_path: "apps/desktop-demo/src/app/controls_ui.rs",
        startup_aliases: &["controls"],
    },
    DemoTabInfo {
        tab: DemoTab::Liquid,
        label: "Liquid UI",
        slug: "liquid-ui",
        source_path: "apps/desktop-demo/src/app/liquid_ui.rs",
        startup_aliases: &["liquid"],
    },
    DemoTabInfo {
        tab: DemoTab::GlassFeed,
        label: "Receipts",
        slug: "glass-feed",
        source_path: "apps/desktop-demo/src/app/glass_feed.rs",
        startup_aliases: &["receipts"],
    },
    DemoTabInfo {
        tab: DemoTab::GlassTiles,
        label: "Glass Tiles",
        slug: "glass-tiles",
        source_path: "apps/desktop-demo/src/app/glass_tiles.rs",
        startup_aliases: &["tiles"],
    },
    DemoTabInfo {
        tab: DemoTab::MarkdownViewer,
        label: "Markdown",
        slug: "markdown-viewer",
        source_path: "apps/desktop-demo/src/app/markdown.rs",
        startup_aliases: &["markdown"],
    },
    DemoTabInfo {
        tab: DemoTab::FloatingWindows,
        label: "Windows",
        slug: "floating-windows",
        source_path: "apps/desktop-demo/src/app/floating_windows.rs",
        startup_aliases: &["windows", "pet", "flame"],
    },
    DemoTabInfo {
        tab: DemoTab::FilePicker,
        label: "File Picker",
        slug: "file-picker",
        source_path: "apps/desktop-demo/src/app.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Rotary,
        label: "Rotary Input",
        slug: "rotary",
        source_path: "apps/desktop-demo/src/app/rotary.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Wear,
        label: "Wear (watch)",
        slug: "wear-watch",
        source_path: "apps/desktop-demo/src/app/wear.rs",
        startup_aliases: &[],
    },
    DemoTabInfo {
        tab: DemoTab::Room,
        label: "Room",
        slug: "room",
        source_path: "apps/desktop-demo/src/app/room.rs",
        startup_aliases: &["spatial"],
    },
];

impl DemoTab {
    fn info(self) -> &'static DemoTabInfo {
        DEMO_TAB_INFO
            .iter()
            .find(|info| info.tab == self)
            .expect("every DemoTab needs a DEMO_TAB_INFO row")
    }

    pub fn label(self) -> &'static str {
        self.info().label
    }

    pub fn slug(self) -> &'static str {
        self.info().slug
    }

    pub fn source_path(self) -> &'static str {
        self.info().source_path
    }

    /// Finds the tab a startup name selects: its slug or one of its aliases,
    /// compared by [`startup_key`].
    pub fn from_startup_name(name: &str) -> Option<Self> {
        let key = startup_key(name).collect::<String>();
        DEMO_TAB_INFO
            .iter()
            .find(|info| {
                startup_key(info.slug).eq(key.chars())
                    || info.startup_aliases.contains(&key.as_str())
            })
            .map(|info| info.tab)
    }
}

/// The characters of a startup name that select a tab or a shader section:
/// ASCII letters and digits, lowercased, so `Glass Tiles`, `glass-tiles` and
/// `glasstiles` name the same tab.
pub fn startup_key(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|ch| ch.to_ascii_lowercase())
}

/// Every page the demo can start on, as the URL query that opens it: each tab
/// in [`DEMO_TABS`] order, then each shader section alone.
#[cfg(any(test, target_arch = "wasm32"))]
pub fn startup_pages() -> impl Iterator<Item = String> {
    let tabs = DEMO_TABS.iter().map(|tab| format!("tab={}", tab.slug()));
    let sections = super::ShaderSection::ALL.iter().map(|section| {
        format!(
            "tab={}&shader_section={}",
            DemoTab::Shaders.slug(),
            startup_key(section.label()).collect::<String>()
        )
    });
    tabs.chain(sections)
}

pub fn startup_tab_from_args(args: impl IntoIterator<Item = String>) -> DemoTab {
    args.into_iter()
        .next()
        .and_then(|name| DemoTab::from_startup_name(&name))
        .unwrap_or(DEFAULT_INITIAL_TAB)
}

pub const DEMO_TABS: [DemoTab; 33] = [
    DemoTab::Guide,
    DemoTab::Performance,
    DemoTab::Liquid,
    DemoTab::Room,
    DemoTab::Gauntlet,
    DemoTab::Counter,
    DemoTab::CompositionLocal,
    DemoTab::Async,
    DemoTab::Animations,
    DemoTab::WebFetch,
    DemoTab::WebView,
    DemoTab::TextInput,
    DemoTab::Layout,
    DemoTab::ModifierShowcase,
    DemoTab::LazyList,
    DemoTab::Mineswapper2,
    DemoTab::RecompositionLab,
    DemoTab::HackerNews,
    DemoTab::Images,
    DemoTab::Text,
    DemoTab::Wsz,
    DemoTab::Xkcd,
    DemoTab::Shaders,
    DemoTab::ShaderRect,
    DemoTab::FloatingWindows,
    DemoTab::Controls,
    DemoTab::MarkdownViewer,
    DemoTab::InteractiveAnim,
    DemoTab::FilePicker,
    DemoTab::Rotary,
    DemoTab::Wear,
    DemoTab::GlassFeed,
    DemoTab::GlassTiles,
];

pub fn demo_tab_labels() -> Vec<&'static str> {
    DEMO_TABS.iter().map(|tab| tab.label()).collect()
}
