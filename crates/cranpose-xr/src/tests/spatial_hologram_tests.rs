// A hologram as a headset runs it: a controller's ray presses the objects of a choice group on the
// hologram's panel and selects their options, a hologram that hid composes nothing, draws nothing
// and asks for no frame, and a crystal button with nothing around it asks for no frame while it
// holds still.

use std::time::{Duration, Instant};

use super::*;
use crate::{ChoiceForm, HologramStage, SpatialChoiceGroup};

// A choice group of `count` options of `form` at the place of `CONTROL`, on a hologram with `stage`.
fn group_bench(form: ChoiceForm, count: usize, stage: HologramStage) -> Bench {
    bench_in(Holder::Hologram(stage), TRAY, move |controls| {
        let [x, y, width, height] = CONTROL;
        SpatialChoiceGroup(
            controls.selected.get(),
            move |index| controls.selected.set(Some(index)),
            count,
            form,
            Modifier::empty().offset(x, y).size_points(width, height),
            |_| {},
        );
    })
}

// The middle of the option `index` of `count` options, which share the group's width.
fn option_at(index: usize, count: usize) -> [f32; 2] {
    let each = CONTROL[2] / count as f32;
    [CONTROL[0] + each * (index as f32 + 0.5), MIDDLE[1]]
}

// A press of the ray at `at`: it rests there while the pointer's smoothing brings the ray to the
// point, pulls the trigger and lets go.
fn press(bench: &mut Bench, at: [f32; 2]) {
    bench.hold(at, 0.0, 400);
    bench.hold(at, 1.0, 150);
    bench.hold(at, 0.0, 150);
}

#[test]
fn a_ray_press_on_an_option_s_object_selects_the_option() {
    for form in [
        ChoiceForm::Cubes,
        ChoiceForm::Gems,
        ChoiceForm::Petals,
        ChoiceForm::Arc,
    ] {
        let mut bench = group_bench(form, 4, HologramStage::NONE);
        press(&mut bench, option_at(2, 4));
        assert_eq!(
            bench.controls.selected.get(),
            Some(2),
            "the third of the {form:?} takes the press"
        );
        press(&mut bench, option_at(0, 4));
        assert_eq!(
            bench.controls.selected.get(),
            Some(0),
            "the first of the {form:?} takes the press"
        );
    }
}

#[test]
fn a_stepper_s_arrows_select_the_options_beside_the_chosen_one_and_stop_at_the_ends() {
    let mut bench = group_bench(ChoiceForm::Stepper, 3, HologramStage::NONE);
    bench.controls.selected.set(Some(1));
    bench.redraw();
    // Each arrow takes 0.2 of the group's weight of 1.4
    let arrow = CONTROL[2] * 0.2 / 1.4;
    let back = [CONTROL[0] + arrow / 2.0, MIDDLE[1]];
    let forward = [CONTROL[0] + CONTROL[2] - arrow / 2.0, MIDDLE[1]];
    press(&mut bench, forward);
    assert_eq!(bench.controls.selected.get(), Some(2), "the arrow forward");
    press(&mut bench, forward);
    assert_eq!(
        bench.controls.selected.get(),
        Some(2),
        "the last option has no option after it"
    );
    press(&mut bench, back);
    press(&mut bench, back);
    assert_eq!(
        bench.controls.selected.get(),
        Some(0),
        "the arrow back, twice"
    );
    // A press on the words in the middle selects nothing
    press(&mut bench, MIDDLE);
    assert_eq!(bench.controls.selected.get(), Some(0), "the middle");
}

#[test]
fn a_hologram_that_hid_composes_nothing_draws_nothing_and_asks_for_no_frame() {
    let mut bench = group_bench(ChoiceForm::Cubes, 4, HologramStage::PAGE);
    bench.redraw();
    assert!(
        bench.headset.guest.shell().has_transient_frame_callbacks(),
        "the clock of a page with a stage runs while it shows"
    );
    bench.controls.visible.set(false);
    let deadline = Instant::now() + Duration::from_secs(1);
    while !bench.headset.space.panels().is_empty() && Instant::now() < deadline {
        bench.draw_once();
    }
    assert!(
        bench.headset.space.panels().is_empty(),
        "the hidden hologram holds no panel and no volume"
    );
    assert_eq!(
        crate::volumes::scene_count(&bench.headset.space),
        0,
        "the frame needs no scene"
    );
    bench.headset.guest.shell().update();
    assert!(
        !bench.headset.guest.shell().has_transient_frame_callbacks(),
        "the clock stopped with the hologram"
    );
}

#[test]
fn a_hologram_leaves_no_depth_in_the_scene_and_a_tray_does() {
    let hologram = group_bench(ChoiceForm::Gems, 3, HologramStage::PAGE);
    assert!(
        !hologram.headset.space.volumes().writes_depth(),
        "the objects of a hologram add light and leave no depth"
    );
    let tray = bench_in(Holder::Tray, TRAY, |_| {});
    assert!(
        tray.headset.space.volumes().writes_depth(),
        "a tray's base and keys leave depth"
    );
}

#[test]
fn a_crystal_button_with_nothing_around_it_asks_for_no_frame_while_it_holds_still() {
    let mut bench = bench_in(
        Holder::Hologram(HologramStage::NONE),
        TRAY,
        move |controls| {
            let [x, y, _, height] = CONTROL;
            SpatialButton(
                move || controls.clicks.set(controls.clicks.get_non_reactive() + 1),
                Modifier::empty().offset(x, y).size_points(height, height),
                true,
                ButtonShape::Ball,
                Color(0.2, 0.6, 1.0, 1.0),
                || {},
            );
        },
    );
    bench.redraw();
    assert!(
        !bench.headset.guest.shell().has_transient_frame_callbacks(),
        "a still crystal runs no clock"
    );
    assert_eq!(
        crate::volumes::scene_count(&bench.headset.space),
        1,
        "the crystal draws in the scene"
    );
    // A press still clicks it
    let middle = [CONTROL[0] + CONTROL[3] / 2.0, MIDDLE[1]];
    press(&mut bench, middle);
    assert_eq!(
        bench.controls.clicks.get(),
        1,
        "the crystal takes the press"
    );
}

// A gem at the start of `CONTROL`, as wide as the control is high, on a hologram with no stage. It
// counts its clicks, and `checked` holds the hover that the app reads from the gem's interaction
// source.
fn gem_bench(enabled: bool) -> Bench {
    bench_in(
        Holder::Hologram(HologramStage::NONE),
        TRAY,
        move |controls| {
            let [x, y, _, height] = CONTROL;
            let source = rememberMutableInteractionSource();
            let hovered = source.collectIsHoveredAsState().get();
            cranpose_core::SideEffect(move || controls.checked.set(hovered));
            crate::SpatialGem(
                move || controls.clicks.set(controls.clicks.get_non_reactive() + 1),
                Modifier::empty().offset(x, y).size_points(height, height),
                enabled,
                Color(0.2, 0.6, 1.0, 1.0),
                crate::GemGlow::Pulse,
                source,
            );
        },
    )
}

#[test]
fn a_ray_press_on_a_gem_clicks_it_and_the_app_reads_the_hover_from_its_source() {
    let mut bench = gem_bench(true);
    let middle = [CONTROL[0] + CONTROL[3] / 2.0, MIDDLE[1]];
    bench.hold(middle, 0.0, 400);
    assert!(
        bench.controls.checked.get(),
        "the app sees the ray that rests on the gem"
    );
    bench.hold(middle, 1.0, 150);
    bench.hold(middle, 0.0, 150);
    assert_eq!(bench.controls.clicks.get(), 1, "the gem takes the press");
    // The ray leaves the gem for the empty end of the hologram
    bench.hold([TRAY[0] - 10.0, MIDDLE[1]], 0.0, 300);
    assert!(
        !bench.controls.checked.get(),
        "the hover ends when the ray leaves"
    );
}

// The box of the bench's one volume, which the projection layer covers.
fn volume_extent(bench: &Bench) -> Option<[f32; 3]> {
    bench
        .headset
        .space
        .volumes()
        .contents()
        .with(|contents| contents.first().and_then(|content| content.draws.extent))
}

#[test]
fn a_page_of_gems_alone_takes_a_box_little_larger_than_itself_once_it_stands() {
    use crate::hologram_stage::{PageObjects, hologram_extent};
    // The bench's hologram is movable: its box holds the handle under it
    let away = [TRAY[0] - 10.0, MIDDLE[1]];
    let mut gem = gem_bench(true);
    gem.hold(away, 0.0, 600);
    assert_eq!(
        volume_extent(&gem),
        Some(hologram_extent(
            TRAY,
            (HologramStage::NONE, true),
            PageObjects::StillGems
        )),
        "a gem turns within its place"
    );
    let mut group = group_bench(ChoiceForm::Gems, 3, HologramStage::NONE);
    group.hold(away, 0.0, 600);
    assert_eq!(
        volume_extent(&group),
        Some(hologram_extent(
            TRAY,
            (HologramStage::NONE, true),
            PageObjects::Any
        )),
        "a chosen option grows beyond its place"
    );
}

#[test]
fn a_disabled_gem_takes_no_hover_and_no_press() {
    let mut bench = gem_bench(false);
    let middle = [CONTROL[0] + CONTROL[3] / 2.0, MIDDLE[1]];
    press(&mut bench, middle);
    assert_eq!(bench.controls.clicks.get(), 0, "no click");
    assert!(!bench.controls.checked.get(), "no hover");
}

#[test]
fn a_gem_turns_while_it_shows_and_its_clock_stops_when_it_hides() {
    let mut bench = gem_bench(true);
    bench.redraw();
    assert!(
        bench.headset.guest.shell().has_transient_frame_callbacks(),
        "the turning gem runs the hologram's clock"
    );
    assert_eq!(
        crate::volumes::scene_count(&bench.headset.space),
        1,
        "the gem draws in the scene"
    );
    bench.controls.visible.set(false);
    let deadline = Instant::now() + Duration::from_secs(1);
    while !bench.headset.space.panels().is_empty() && Instant::now() < deadline {
        bench.draw_once();
    }
    bench.headset.guest.shell().update();
    assert!(
        bench.headset.space.panels().is_empty()
            && !bench.headset.guest.shell().has_transient_frame_callbacks(),
        "a hidden gem holds no panel and runs no clock"
    );
}
