use super::*;

thread_local! {
    static DRAW_VALUES: RefCell<Option<[MutableState<f32>; 2]>> = const { RefCell::new(None) };
}

#[composable]
fn IndependentDraws(layers: bool) {
    let first = rememberMutableStateOf(|| 20.0);
    let second = rememberMutableStateOf(|| 30.0);
    DRAW_VALUES.with(|slot| *slot.borrow_mut() = Some([first, second]));
    let modifier = Modifier::empty().size_points(100.0, 80.0).focusable();
    let modifier = if layers {
        modifier
            .graphics_layer(move || GraphicsLayer {
                translation_x: first.get(),
                ..Default::default()
            })
            .graphics_layer(move || GraphicsLayer {
                translation_y: second.get(),
                ..Default::default()
            })
            .background(Color::WHITE)
    } else {
        modifier
            .draw_behind(move |scope| {
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: first.get(),
                        height: 10.0,
                    },
                    Brush::solid(Color::WHITE),
                );
            })
            .draw_behind(move |scope| {
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: 20.0,
                        width: second.get(),
                        height: 10.0,
                    },
                    Brush::solid(Color::BLACK),
                );
            })
    };
    Box(modifier, BoxSpec::default(), || {});
}

fn assert_independent_draws(layers: bool) {
    let _guard = test_guard();
    let mut host = CountedShell::settled(location_key(file!(), line!(), column!()), move || {
        IndependentDraws(layers);
    });
    host.frame();
    let states = DRAW_VALUES
        .with(|slot| *slot.borrow())
        .expect("draw states are published");
    for state in states.into_iter().cycle().take(4) {
        state.set_value(state.get_non_reactive() + 5.0);
        host.shell.surfaces[0].renderer.visual_updates.set(0);
        assert_eq!(
            host.frame(),
            (0, 0),
            "draw changes must avoid structural scene updates"
        );
        assert_eq!(
            host.shell.surfaces[0].renderer.visual_updates.get(),
            1,
            "each modifier must schedule its own draw update"
        );
        assert_scene_matches_rebuild(&mut host.shell, "independent draw observation");
    }
    assert_eq!(host.frame(), (0, 0), "unchanged draws must remain idle");
}

#[test]
fn independent_draw_commands_keep_their_reads_beneath_a_focus_ring() {
    assert_independent_draws(false);
}

#[test]
fn independent_graphics_layers_keep_their_reads_on_one_node() {
    assert_independent_draws(true);
}
