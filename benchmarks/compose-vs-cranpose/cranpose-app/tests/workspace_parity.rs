use cranpose_app_shell::AppShell;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_testing::robot_assertions::{
    find_semantics_node as find, semantics_contain_text as contains_text,
};
use cranpose_ui::{SemanticsRole, SemanticsTree};
use perf_compare::{WorkspaceFrame, WorkspaceMode};

struct WorkspaceHarness {
    shell: AppShell<PixelsRenderer>,
}

impl WorkspaceHarness {
    fn new() -> Self {
        Self::with_mode(WorkspaceMode::Quotes, true)
    }

    fn with_mode(mode: WorkspaceMode, still: bool) -> Self {
        let mut shell = AppShell::new_with_size(
            PixelsRenderer::new(),
            cranpose_core::location_key(file!(), line!(), column!()),
            move || WorkspaceFrame(mode, still),
            (1280, 820),
            (1280.0, 820.0),
        );
        shell.set_semantics_enabled(true);
        Self { shell }
    }

    fn semantics(&mut self) -> &SemanticsTree {
        self.shell.update();
        self.shell.semantics_tree().expect("workspace semantics")
    }

    fn click(&mut self, label: &str) {
        let semantics = self.semantics();
        let control = find(semantics.root(), &|node| {
            !node.actions.is_empty() && contains_text(node, label)
        })
        .unwrap_or_else(|| panic!("{label} has an activation action"))
        .node_id;
        assert!(
            self.shell.accessibility_activate(control, None),
            "{label} activates"
        );
    }

    fn pixel(&mut self, x: usize, y: usize) -> [u8; 4] {
        let mut image = vec![0; 1280 * 820 * 4];
        let context = std::rc::Rc::clone(self.shell.app_context());
        context.enter(|| self.shell.renderer().draw(&mut image, 1280, 820));
        image[(y * 1280 + x) * 4..][..4]
            .try_into()
            .expect("RGBA pixel")
    }

    fn quote_price(&mut self) -> String {
        let tree = self.semantics();
        let price = find(tree.root(), &|node| {
            node.description.as_deref() == Some("Selected quote price")
        })
        .expect("visible quote price");
        match &price.role {
            SemanticsRole::Text { value } => value.as_str().to_owned(),
            _ => panic!("quote price is text"),
        }
    }

    fn row_y(&mut self) -> f32 {
        let tree = self.semantics();
        find(tree.root(), &|node| {
            node.description.as_deref() == Some("Watchlist row 4")
        })
        .expect("visible watchlist row")
        .bounds
        .y
    }

    fn assert_search_text(&mut self, expected: &str) {
        let tree = self.semantics();
        let field =
            find(tree.root(), &|node| node.details().editable_text).expect("editable search");
        assert_eq!(field.text.as_deref(), Some(expected));
    }
}

#[test]
fn workspace_shortcuts_control_the_workspace_while_search_has_focus() {
    use cranpose_ui::{KeyCode, KeyEvent};
    let mut app = WorkspaceHarness::new();
    app.click("Search symbols");
    assert!(app.shell.on_paste("AAPL"));
    for (key, text, label) in [
        (KeyCode::Digit2, "2", "Auto-scroll Sidebar"),
        (KeyCode::Digit6, "6", "Auto-scroll Watchlist"),
        (KeyCode::Digit1, "1", "Auto-scroll Off"),
    ] {
        assert!(app.shell.on_key_event(&KeyEvent::key_down(key, text)));
        let tree = app.semantics();
        let control = find(tree.root(), &|node| {
            node.description.as_deref() == Some(label)
        })
        .expect("scroll control");
        assert_eq!(control.selected, Some(true));
    }
    for enabled in [true, false] {
        assert!(app.shell.on_key_event(&KeyEvent::key_down(KeyCode::Q, "q")));
        let tree = app.semantics();
        let control = find(tree.root(), &|node| {
            node.description.as_deref() == Some("Stream quotes") && node.toggled.is_some()
        })
        .expect("stream control");
        assert_eq!(control.toggled, Some(enabled));
    }
    app.assert_search_text("AAPL");
}

#[test]
fn workspace_sidebar_shows_reference_navigation() {
    let mut app = WorkspaceHarness::new();
    let semantics = app.semantics();
    for name in [
        "Introduction",
        "Installation",
        "Theming",
        "Components",
        "Accordion",
        "Alert",
    ] {
        assert!(
            contains_text(semantics.root(), name),
            "sidebar shows {name}"
        );
    }
    assert!(!contains_text(semantics.root(), "Accordion 1"));
}

#[test]
fn workspace_dock_tabs_switch_content_and_restore_the_quote() {
    let mut app = WorkspaceHarness::new();
    app.click("Profile");
    let profile = app.semantics();
    assert!(contains_text(profile.root(), "Profile panel"));
    assert!(contains_text(profile.root(), "0 updates"));
    assert!(!contains_text(profile.root(), "Selected quote price"));
    app.click("Quote");
    let quote = app.semantics();
    assert!(contains_text(quote.root(), "Summit Logistics"));
    assert!(!contains_text(quote.root(), "Profile panel"));
}

#[test]
fn workspace_controls_expose_their_current_state() {
    let mut app = WorkspaceHarness::new();
    app.click("Stream quotes");
    app.click("Stream quotes");
    let paused = app.semantics();
    let streaming = find(paused.root(), &|node| {
        node.description.as_deref() == Some("Stream quotes") && node.toggled.is_some()
    })
    .expect("quote stream switch");
    assert_eq!(streaming.toggled, Some(false));
    app.click("Auto-scroll Watchlist");
    let scrolling = app.semantics();
    let watchlist = find(scrolling.root(), &|node| {
        node.description.as_deref() == Some("Auto-scroll Watchlist")
    })
    .expect("watchlist scrolling control");
    assert_eq!(watchlist.selected, Some(true));
    app.click("Auto-scroll Off");
    let stopped = app.semantics();
    let off = find(stopped.root(), &|node| {
        node.description.as_deref() == Some("Auto-scroll Off")
    })
    .expect("stop scrolling control");
    assert_eq!(off.selected, Some(true));
}

#[test]
fn workspace_search_accepts_typed_text() {
    let mut app = WorkspaceHarness::new();
    let tree = app.semantics();
    let search = find(tree.root(), &|node| {
        node.details().editable_text && contains_text(node, "Search symbols")
    })
    .expect("editable symbol search")
    .node_id;
    assert!(app.shell.accessibility_activate(search, None));
    assert!(app.shell.on_paste("AAPL"));
    app.assert_search_text("AAPL");
}

#[test]
fn workspace_hover_moves_between_visible_rows() {
    let mut app = WorkspaceHarness::with_mode(WorkspaceMode::Hover, false);
    app.shell
        .update_after_exact_interval(std::time::Duration::ZERO);
    let tree = app.shell.semantics_tree().expect("workspace semantics");
    let row = find(tree.root(), &|node| {
        node.description.as_deref() == Some("Watchlist row 0")
    })
    .expect("first visible row");
    let x = (row.bounds.x + 2.0) as usize;
    let y = (row.bounds.y + 2.0) as usize;
    let before = app.pixel(x, y);
    app.shell
        .update_after_exact_interval(std::time::Duration::from_millis(16));
    app.shell
        .update_after_exact_interval(std::time::Duration::from_millis(16));
    let highlighted = app.pixel(x, y);
    assert_ne!(
        highlighted, before,
        "first real mouse sample highlights the first row"
    );
    app.shell
        .update_after_exact_interval(std::time::Duration::from_millis(16));
    assert_eq!(
        app.pixel(x, y),
        before,
        "next mouse sample leaves the first row"
    );
}

#[test]
fn workspace_stream_control_freezes_and_restarts_quotes() {
    let mut app = WorkspaceHarness::new();
    let initial = app.quote_price();
    app.click("Stream quotes");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while app.quote_price() == initial && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_ne!(
        app.quote_price(),
        initial,
        "enabled stream updates the quote"
    );
    app.click("Stream quotes");
    let frozen = app.quote_price();
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(
            app.quote_price(),
            frozen,
            "disabled stream leaves the quote unchanged"
        );
    }
}

#[test]
fn workspace_scroll_control_moves_rows_then_stops() {
    let mut app = WorkspaceHarness::new();
    let initial = app.row_y();
    app.click("Auto-scroll Watchlist");
    for _ in 0..3 {
        app.shell
            .update_after_exact_interval(std::time::Duration::from_millis(16));
    }
    let moved = app.row_y();
    assert!(
        moved < initial,
        "watchlist rows move up when scrolling starts"
    );
    app.click("Auto-scroll Off");
    let stopped = app.row_y();
    for _ in 0..3 {
        app.shell
            .update_after_exact_interval(std::time::Duration::from_millis(16));
    }
    assert_eq!(
        app.row_y(),
        stopped,
        "watchlist rows stop when Off is selected"
    );
}
