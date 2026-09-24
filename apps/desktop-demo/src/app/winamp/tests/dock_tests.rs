use std::{cell::Cell, sync::Arc};

use cranpose_core::{DefaultScheduler, NodeId, Runtime};
use cranpose_ui::TestComposition;

use super::*;

#[test]
fn a_pane_stays_in_the_stack_until_the_title_has_really_been_carried_off() {
    assert!(
        !pane_left_the_stack(Point::new(4.0, 6.0)),
        "the press that means to move the whole stack wanders a few pixels, and tearing \
         a pane off every time it does would make the window impossible to drag"
    );
    assert!(pane_left_the_stack(Point::new(0.0, 40.0)));
    assert!(pane_left_the_stack(Point::new(-40.0, 0.0)));
}

#[test]
fn a_pane_docks_when_its_top_edge_meets_the_bottom_of_the_stack() {
    let slot = Point::new(140.0, 497.0);
    assert!(
        pane_came_back_to_its_slot(Point::new(153.0, 497.0), slot),
        "a pane let go with its top edge on the stack's bottom edge and a little to one side \
         is meant for the stack; asking for the sideways pixel as well would mean the pane \
         almost never goes back in"
    );
    assert!(pane_came_back_to_its_slot(slot, slot));
    assert!(
        pane_came_back_to_its_slot(Point::new(255.0, 540.0), Point::new(240.0, 550.0)),
        "a pane let go ten pixels short of the stack was meant for it"
    );
    assert!(
        !pane_came_back_to_its_slot(Point::new(140.0, 497.0 + MAIN_HEIGHT), slot),
        "a pane a whole pane's height below the stack was put down, not docked"
    );
    assert!(
        !pane_came_back_to_its_slot(Point::new(140.0 + MAIN_WIDTH, 497.0), slot),
        "a pane beside the stack rather than under it is not in it"
    );
}

#[test]
fn a_torn_pane_is_the_only_one_that_leaves_the_main_window() {
    let mut dock = WinampDock::default();
    assert!(dock.docked(WinampPane::Equalizer));
    assert!(dock.docked(WinampPane::Playlist));
    dock.tear(WinampPane::Equalizer);
    assert!(!dock.docked(WinampPane::Equalizer));
    assert!(
        dock.docked(WinampPane::Playlist),
        "pulling one pane out leaves the rest of the stack where it was"
    );
    dock.tear(WinampPane::Equalizer);
    dock.dock(WinampPane::Equalizer);
    assert!(
        dock.docked(WinampPane::Equalizer),
        "a pane torn twice is still one pane, so docking it once puts it back"
    );
}

fn docked_at(y: f32) -> Option<WinampPaneSlot> {
    Some(WinampPaneSlot {
        docked: true,
        offset: Point::new(0.0, y),
    })
}

fn torn_from(y: f32) -> Option<WinampPaneSlot> {
    Some(WinampPaneSlot {
        docked: false,
        offset: Point::new(0.0, y),
    })
}

#[test]
fn the_stack_is_as_big_as_the_panes_docked_in_it() {
    let shown = WinampState::default();
    let playlist = Size::new(PLAYLIST_WIDTH, PLAYLIST_HEIGHT);
    let mut dock = WinampDock::default();

    assert_eq!(
        winamp_stack_layout(&shown, &dock, playlist),
        WinampStackLayout {
            equalizer: docked_at(MAIN_HEIGHT),
            playlist: docked_at(MAIN_HEIGHT + EQ_HEIGHT),
            size: Size::new(MAIN_WIDTH, MAIN_HEIGHT + EQ_HEIGHT + PLAYLIST_HEIGHT),
        },
        "the main window, the equalizer under it and the playlist under that"
    );

    dock.tear(WinampPane::Equalizer);
    assert_eq!(
        winamp_stack_layout(&shown, &dock, playlist),
        WinampStackLayout {
            equalizer: torn_from(MAIN_HEIGHT),
            playlist: docked_at(MAIN_HEIGHT),
            size: Size::new(MAIN_WIDTH, MAIN_HEIGHT + PLAYLIST_HEIGHT),
        },
        "a torn equalizer leaves the stack, and the playlist moves up into its place"
    );

    dock.tear(WinampPane::Playlist);
    assert_eq!(
        winamp_stack_layout(&shown, &dock, playlist).size,
        Size::new(MAIN_WIDTH, MAIN_HEIGHT),
        "with both panes torn the stack is the main window alone"
    );

    let hidden_equalizer = WinampState {
        eq_visible: false,
        ..WinampState::default()
    };
    let wide_playlist = Size::new(PLAYLIST_WIDTH + 75.0, PLAYLIST_HEIGHT + 58.0);
    assert_eq!(
        winamp_stack_layout(&hidden_equalizer, &WinampDock::default(), wide_playlist),
        WinampStackLayout {
            equalizer: None,
            playlist: docked_at(MAIN_HEIGHT),
            size: Size::new(wide_playlist.width, MAIN_HEIGHT + wide_playlist.height),
        },
        "a hidden pane takes no room, and a docked playlist wider than the main window \
         widens the stack"
    );
}

fn torn(
    pane: WinampPane,
    state: WindowState,
    main_window: WindowState,
    dock: MutableState<WinampDock>,
) -> WinampStackPane {
    WinampStackPane {
        pane,
        dock,
        offset: Point::new(0.0, MAIN_HEIGHT),
        main_window,
        state,
    }
}

#[test]
fn a_pane_let_go_on_the_bottom_of_the_stack_goes_back_in() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let main_window =
        WindowState::placed_at(100.0, 100.0, MAIN_WIDTH, MAIN_HEIGHT + PLAYLIST_HEIGHT);
    let equalizer = WindowState::placed_at(112.0, 424.0, EQ_WIDTH, EQ_HEIGHT);
    let dock = cranpose_core::mutableStateOf(WinampDock {
        torn: vec![WinampPane::Equalizer],
    });

    dock_a_pane_let_go_on_the_stack(torn(WinampPane::Equalizer, equalizer, main_window, dock));

    assert!(
        dock.get_non_reactive().docked(WinampPane::Equalizer),
        "the stack's bottom edge is as far down as the main window is tall"
    );
}

#[test]
fn a_pane_let_go_away_from_the_stack_stays_in_its_own_window() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let main_window = WindowState::placed_at(100.0, 100.0, MAIN_WIDTH, MAIN_HEIGHT);
    let dock = cranpose_core::mutableStateOf(WinampDock {
        torn: vec![WinampPane::Equalizer, WinampPane::Playlist],
    });

    let far_off = WindowState::placed_at(600.0, 100.0, EQ_WIDTH, EQ_HEIGHT);
    dock_a_pane_let_go_on_the_stack(torn(WinampPane::Equalizer, far_off, main_window, dock));
    let nowhere = WindowState::new(PLAYLIST_WIDTH, PLAYLIST_HEIGHT);
    dock_a_pane_let_go_on_the_stack(torn(WinampPane::Playlist, nowhere, main_window, dock));

    assert_eq!(
        dock.get_non_reactive(),
        WinampDock {
            torn: vec![WinampPane::Equalizer, WinampPane::Playlist],
        },
        "a pane put down away from the stack, or with no place on the screen yet, is not docked"
    );
}

#[test]
fn the_playlist_corner_stretches_it_but_never_below_the_size_the_skin_draws() {
    let held = Size::new(PLAYLIST_WIDTH, PLAYLIST_HEIGHT);
    let composition = cranpose_ui::run_test_composition(|| {});
    composition.with_app_context(|| {
        assert_eq!(
            stretched_playlist(held, Point::new(30.4, 40.0), 1.0),
            Size::new(PLAYLIST_WIDTH + 30.0, PLAYLIST_HEIGHT + 40.0),
            "the corner lands on whole pixels"
        );
        assert_eq!(
            stretched_playlist(held, Point::new(-30.0, -40.0), 1.0),
            held,
            "the skin has no picture for a playlist smaller than its own"
        );
    });
}

#[test]
fn a_playlist_window_is_measured_in_skin_pixels() {
    assert_eq!(
        playlist_skin_size(Size::new(PLAYLIST_WIDTH * 2.0 + 50.0, 20.0), 2.0),
        Size::new(PLAYLIST_WIDTH + 25.0, PLAYLIST_HEIGHT),
        "a window's size is read in skin pixels, and never below the skin's own"
    );
}

fn window_root_nodes() -> Vec<NodeId> {
    cranpose_ui::window_roots()
        .into_iter()
        .map(|entry| entry.node)
        .collect()
}

fn settle(composition: &mut TestComposition) {
    while composition
        .process_invalid_scopes()
        .expect("the Winamp tab recomposes")
    {}
}

fn undocked_winamp_tab() -> (TestComposition, WinampTabState) {
    let tab = Rc::new(Cell::new(None));
    let mut composition = cranpose_ui::run_test_composition({
        let tab = Rc::clone(&tab);
        move || {
            let tab_state = remember_winamp_tab_state();
            tab.set(Some(tab_state));
            WinampTab(tab_state);
        }
    });
    let tab_state = tab.get().expect("the Winamp tab composed");
    tab_state.detached.set(true);
    settle(&mut composition);
    (composition, tab_state)
}

#[test]
fn the_panes_draw_inside_the_main_window_until_one_is_torn_off() {
    let (mut composition, tab) = undocked_winamp_tab();
    let stack = window_root_nodes();
    assert_eq!(
        stack.len(),
        1,
        "the main window holds the equalizer and the playlist docked under it"
    );
    assert_eq!(
        tab.peer_windows.main.size_non_reactive(),
        Size::new(MAIN_WIDTH, MAIN_HEIGHT + EQ_HEIGHT + PLAYLIST_HEIGHT),
        "the main window is the size of the stack"
    );

    tab.dock.update(|held| held.tear(WinampPane::Equalizer));
    settle(&mut composition);
    let torn = window_root_nodes();
    assert_eq!(torn.len(), 2, "the torn equalizer is a window of its own");
    assert_eq!(torn[0], stack[0], "the stack keeps its window");
    assert_eq!(
        tab.peer_windows.main.size_non_reactive(),
        Size::new(MAIN_WIDTH, MAIN_HEIGHT + PLAYLIST_HEIGHT),
        "the stack gives up the torn equalizer's room"
    );

    tab.dock.update(|held| held.dock(WinampPane::Equalizer));
    settle(&mut composition);
    assert_eq!(window_root_nodes(), stack);
    assert_eq!(
        tab.peer_windows.main.size_non_reactive(),
        Size::new(MAIN_WIDTH, MAIN_HEIGHT + EQ_HEIGHT + PLAYLIST_HEIGHT),
        "a docked equalizer takes its room in the stack back"
    );

    tab.dock.update(|held| held.tear(WinampPane::Equalizer));
    settle(&mut composition);
    assert_eq!(
        window_root_nodes(),
        torn,
        "the equalizer is composed once, in the stack: tearing it again puts the same node \
         in a window, so it keeps its state and the window the desktop put away"
    );
}

#[test]
fn undocking_makes_the_same_nodes_windows_again() {
    let (mut composition, tab) = undocked_winamp_tab();
    tab.dock.update(|held| held.tear(WinampPane::Playlist));
    settle(&mut composition);
    let undocked = window_root_nodes();
    assert_eq!(
        undocked.len(),
        2,
        "the stack with the equalizer in it, and the torn playlist"
    );

    tab.detached.set(false);
    settle(&mut composition);
    assert!(
        window_root_nodes().is_empty(),
        "docked in the tab, everything draws inline"
    );

    tab.detached.set(true);
    settle(&mut composition);
    assert_eq!(
        window_root_nodes(),
        undocked,
        "a window is the node that carries it, so the desktop shows the windows docking put \
         away only when undocking makes the same nodes windows again"
    );
}
