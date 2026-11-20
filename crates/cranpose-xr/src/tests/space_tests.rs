use std::cell::{Cell, RefCell};

use cranpose::prelude::*;
use cranpose_core::{CompositionLocalProvider, mutableStateOf, remember, rememberMutableStateOf};
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    ResizeLimits, ResizePolicy, SpatialPanel, SpatialResizeEventType, SubspaceModifier,
    space::PanelResize,
};

fn with_space(test: impl FnOnce(Space)) {
    cranpose_testing::run_test_composition(|_| test(Space::new()));
}

// The part of the atlas that the panel `id` has now.
fn part(space: &Space, id: u64) -> [f32; 4] {
    space
        .panels()
        .iter()
        .find(|slot| slot.id == id)
        .expect("the panel is registered")
        .atlas
}

#[test]
fn panels_take_parts_of_the_atlas_one_under_the_other() {
    with_space(|space| {
        let (first, second) = (space.new_panel(), space.new_panel());
        assert_eq!(
            space.place(first, [800.0, 500.0], Pose::IDENTITY, Pose::IDENTITY),
            [0.0, 0.0, 800.0, 500.0]
        );
        assert_eq!(
            space.place(second, [300.0, 200.0], Pose::IDENTITY, Pose::IDENTITY),
            [0.0, 508.0, 300.0, 200.0]
        );
        assert_eq!(space.atlas_size(), [800.0, 708.0]);
    });
}

#[test]
fn a_surface_takes_no_room_in_the_atlas() {
    with_space(|space| {
        let (first, video, second) = (space.new_panel(), space.new_panel(), space.new_panel());
        space.place(first, [800.0, 500.0], Pose::IDENTITY, Pose::IDENTITY);
        space.place_surface(
            video,
            StereoMode::Mono,
            [640.0, 360.0],
            Pose::IDENTITY,
            Pose::IDENTITY,
        );
        assert_eq!(
            space.place(second, [300.0, 200.0], Pose::IDENTITY, Pose::IDENTITY),
            [0.0, 508.0, 300.0, 200.0]
        );
        assert_eq!(space.atlas_size(), [800.0, 708.0]);
        assert_eq!(space.panels()[1].size(), [640.0, 360.0]);
        space.remove(video.id);
        assert_eq!(space.panels().len(), 2);
    });
}

#[test]
fn a_panel_keeps_its_part_while_its_size_stays() {
    with_space(|space| {
        let (first, second) = (space.new_panel(), space.new_panel());
        space.place(first, [400.0, 300.0], Pose::IDENTITY, Pose::IDENTITY);
        let part = space.place(second, [400.0, 100.0], Pose::IDENTITY, Pose::IDENTITY);
        let moved = Pose::at([0.1, 0.0, -1.0]);
        assert_eq!(
            space.place(second, [400.0, 100.0], Pose::IDENTITY, moved),
            part
        );
        assert_eq!(space.panels()[1].pose, moved);
        assert_eq!(
            space.place(first, [400.0, 300.0], Pose::IDENTITY, moved),
            [0.0, 0.0, 400.0, 300.0]
        );
        assert_eq!(
            space.place(second, [400.0, 100.0], Pose::IDENTITY, moved),
            part
        );
    });
}

#[test]
fn a_panel_that_changes_its_size_moves_the_panels_under_it() {
    with_space(|space| {
        let (first, second) = (space.new_panel(), space.new_panel());
        space.place(first, [400.0, 300.0], Pose::IDENTITY, Pose::IDENTITY);
        space.place(second, [400.0, 100.0], Pose::IDENTITY, Pose::IDENTITY);
        assert_eq!(
            space.place(first, [400.0, 320.0], Pose::IDENTITY, Pose::IDENTITY),
            [0.0, 0.0, 400.0, 320.0]
        );
        assert_eq!(part(&space, second.id), [0.0, 328.0, 400.0, 100.0]);
        assert_eq!(second.origin.get_non_reactive(), [0.0, 328.0]);
        assert_eq!(first.origin.get_non_reactive(), [0.0, 0.0]);
    });
}

#[test]
fn many_resizes_keep_the_atlas_height_bounded() {
    with_space(|space| {
        let (first, second) = (space.new_panel(), space.new_panel());
        space.place(first, [400.0, 100.0], Pose::IDENTITY, Pose::IDENTITY);
        space.place(second, [400.0, 100.0], Pose::IDENTITY, Pose::IDENTITY);
        for step in 0..2000 {
            let height = 100.0 + (step % 50) as f32 * 4.0;
            space.place(first, [400.0, height], Pose::IDENTITY, Pose::IDENTITY);
            space.place(
                second,
                [400.0, 100.0 + (step % 7) as f32],
                Pose::IDENTITY,
                Pose::IDENTITY,
            );
            // The first panel is at most 296 dp tall. The second one is at most 106 dp tall.
            assert!(space.atlas_size()[1] <= 296.0 + ATLAS_GAP + 106.0);
        }
        let [_, top, _, _] = part(&space, second.id);
        assert_eq!(top, 304.0);
    });
}

#[test]
fn a_removed_panel_leaves_no_gap() {
    with_space(|space| {
        let panels = [space.new_panel(), space.new_panel(), space.new_panel()];
        for (panel, height) in panels.iter().zip([300.0, 200.0, 100.0]) {
            space.place(*panel, [400.0, height], Pose::IDENTITY, Pose::IDENTITY);
        }
        assert_eq!(
            space.atlas_size(),
            [400.0, 300.0 + 8.0 + 200.0 + 8.0 + 100.0]
        );
        space.remove(panels[1].id);
        assert_eq!(part(&space, panels[0].id), [0.0, 0.0, 400.0, 300.0]);
        assert_eq!(part(&space, panels[2].id), [0.0, 308.0, 400.0, 100.0]);
        assert_eq!(panels[2].origin.get_non_reactive(), [0.0, 308.0]);
        assert_eq!(space.atlas_size(), [400.0, 408.0]);
        space.remove(panels[0].id);
        space.remove(panels[2].id);
        assert!(space.panels().is_empty());
        assert_eq!(space.atlas_size(), [0.0, 0.0]);
        let again = space.new_panel();
        assert_eq!(
            space.place(again, [10.0, 10.0], Pose::IDENTITY, Pose::IDENTITY),
            [0.0, 0.0, 10.0, 10.0]
        );
    });
}

#[test]
fn the_cursor_shows_on_its_panel_only() {
    with_space(|space| {
        let (first, second) = (space.new_panel(), space.new_panel());
        space.place(first, [400.0, 300.0], Pose::IDENTITY, Pose::IDENTITY);
        space.place(second, [400.0, 300.0], Pose::IDENTITY, Pose::IDENTITY);
        let on = |panel: Panel, x: f32, pressed: bool| Cursor {
            panel: panel.id,
            x,
            y: 20.0,
            pressed,
        };
        space.set_cursor(Some(on(first, 10.0, false)));
        let dot = Dot {
            x: 10.0,
            y: 20.0,
            pressed: false,
        };
        assert_eq!(first.dot.get_non_reactive(), Some(dot));
        assert_eq!(second.dot.get_non_reactive(), None);
        space.set_cursor(Some(on(second, 10.0, true)));
        assert_eq!(first.dot.get_non_reactive(), None);
        assert_eq!(
            second.dot.get_non_reactive(),
            Some(Dot {
                pressed: true,
                ..dot
            })
        );
        space.set_cursor(None);
        assert_eq!(second.dot.get_non_reactive(), None);
    });
}

#[test]
fn the_cursor_moves_in_steps_of_half_a_dp() {
    with_space(|space| {
        let panel = space.new_panel();
        space.place(panel, [400.0, 300.0], Pose::IDENTITY, Pose::IDENTITY);
        let at = |x: f32, y: f32| {
            Some(Cursor {
                panel: panel.id,
                x,
                y,
                pressed: false,
            })
        };
        let dot = |x: f32, y: f32| {
            Some(Dot {
                x,
                y,
                pressed: false,
            })
        };
        space.set_cursor(at(10.1, 20.2));
        assert_eq!(panel.dot.get_non_reactive(), dot(10.0, 20.0));
        space.set_cursor(at(9.9, 20.24));
        assert_eq!(panel.dot.get_non_reactive(), dot(10.0, 20.0));
        space.set_cursor(at(10.3, 20.3));
        assert_eq!(panel.dot.get_non_reactive(), dot(10.5, 20.5));
    });
}

// Two spatial panels of a headset's composition. The first one can change its size or leave.
// The second one counts its compositions and its clicks.
struct Headset {
    robot: RobotTestRule<TestRenderer>,
    space: Space,
    first: MutableState<Option<[f32; 2]>>,
    compositions: Rc<Cell<u32>>,
    clicks: Rc<Cell<u32>>,
}

// The first panel reads its size in its own scope, so a change of the size composes no other panel.
#[composable]
fn FirstPanel(size: MutableState<Option<[f32; 2]>>) {
    if let Some([width, height]) = size.get() {
        SpatialPanel(SubspaceModifier::new().size(width, height), || {});
    }
}

fn headset() -> Headset {
    let compositions = Rc::new(Cell::new(0));
    let clicks = Rc::new(Cell::new(0));
    let held = Rc::new(RefCell::new(None));
    let (counted, clicked, keep) = (
        Rc::clone(&compositions),
        Rc::clone(&clicks),
        Rc::clone(&held),
    );
    let robot = create_headless_robot_test(500, 1000, move || {
        let space = remember(Space::new).with(Clone::clone);
        let first = rememberMutableStateOf(|| Some([400.0, 300.0]));
        keep.replace(Some((space.clone(), first)));
        let (counted, clicked) = (Rc::clone(&counted), Rc::clone(&clicked));
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            FirstPanel(first);
            let (counted, clicked) = (Rc::clone(&counted), Rc::clone(&clicked));
            SpatialPanel(SubspaceModifier::new().size(400.0, 100.0), move || {
                counted.set(counted.get() + 1);
                let clicked = Rc::clone(&clicked);
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .clickable(move |_| clicked.set(clicked.get() + 1)),
                    BoxSpec::default(),
                    || {},
                );
            });
        });
    });
    let (space, first) = held.take().expect("the content composed");
    Headset {
        robot,
        space,
        first,
        compositions,
        clicks,
    }
}

impl Headset {
    // Whether a click at `y` dp in the atlas, near its left edge, reaches the second panel.
    fn reaches_second(&mut self, y: f32) -> bool {
        let before = self.clicks.get();
        self.robot.click_at(5.0, y);
        self.clicks.get() > before
    }

    fn second(&self) -> PanelSlot {
        *self.space.panels().last().expect("the second panel stays")
    }
}

#[test]
fn a_panel_follows_the_panels_above_it_without_composing_again() {
    let mut headset = headset();
    headset.robot.wait_for_idle();
    assert_eq!(headset.second().atlas, [0.0, 308.0, 400.0, 100.0]);
    assert!(headset.reaches_second(350.0));
    let compositions = headset.compositions.get();

    headset.first.set(Some([400.0, 200.0]));
    headset.robot.wait_for_idle();
    assert_eq!(headset.second().atlas, [0.0, 208.0, 400.0, 100.0]);
    assert!(headset.reaches_second(250.0), "the panel moved up");
    assert!(!headset.reaches_second(350.0), "the panel left its place");

    headset.first.set(None);
    headset.robot.wait_for_idle();
    assert_eq!(headset.second().atlas, [0.0, 0.0, 400.0, 100.0]);
    assert!(headset.reaches_second(50.0), "the panel took the top");
    assert!(!headset.reaches_second(250.0));
    assert_eq!(
        headset.compositions.get(),
        compositions,
        "the move is read where the scene draws"
    );
}

#[test]
fn a_moving_cursor_does_not_compose_the_content_again() {
    let mut headset = headset();
    headset.robot.wait_for_idle();
    let [first, second] = [0, 1].map(|index| headset.space.panels()[index].id);
    let compositions = headset.compositions.get();
    let on = |panel: u64, x: f32, pressed: bool| {
        Some(Cursor {
            panel,
            x,
            y: 40.0,
            pressed,
        })
    };
    let before = headset.robot.get_all_rects().len();

    headset.space.set_cursor(on(second, 10.0, false));
    headset.robot.wait_for_idle();
    assert_eq!(headset.robot.get_all_rects().len(), before + 1, "the dot");
    for step in 0..40 {
        let x = 10.0 + step as f32 * 0.1;
        headset.space.set_cursor(on(second, x, false));
        headset
            .space
            .set_cursor(on(second, x + 20.0, step % 2 == 0));
        headset.space.set_cursor(on(first, x, false));
        headset.robot.wait_for_idle();
    }
    headset.space.set_cursor(None);
    headset.robot.wait_for_idle();
    assert_eq!(headset.robot.get_all_rects().len(), before, "no dot");
    assert_eq!(headset.compositions.get(), compositions);
}

#[test]
fn a_panel_can_leave_in_the_frame_its_cursor_moves() {
    for cursor_first in [true, false] {
        let mut headset = headset();
        headset.robot.wait_for_idle();
        let panel = headset.space.panels()[0].id;
        let on = |x: f32| {
            Some(Cursor {
                panel,
                x,
                y: 40.0,
                pressed: x > 100.0,
            })
        };
        headset.space.set_cursor(on(10.0));
        headset.robot.wait_for_idle();
        if cursor_first {
            headset.space.set_cursor(on(120.0));
            headset.first.set(None);
        } else {
            headset.first.set(None);
            headset.space.set_cursor(on(120.0));
        }
        headset.robot.wait_for_idle();
        headset.space.set_cursor(on(130.0));
        headset.robot.wait_for_idle();
        headset.space.set_cursor(None);
        headset.robot.wait_for_idle();
        assert_eq!(headset.space.panels().len(), 1);
    }
}

#[test]
fn a_carried_panel_keeps_the_pose_the_user_gave() {
    with_space(|space| {
        let panel = space.new_panel();
        space.place(panel, [400.0, 300.0], Pose::IDENTITY, Pose::IDENTITY);
        space.set_move_policy(panel.id, Some(&MovePolicy::default()));
        assert_eq!(space.panels()[0].mobility, Mobility::Movable);

        let carried = Pose::at([0.5, 0.2, -1.0]);
        space.carry(&PanelMove {
            panel: panel.id,
            event_type: SpatialMoveEventType::Moving,
            pose: carried,
        });
        assert_eq!(space.panels()[0].pose, carried);

        let part = space.place(
            panel,
            [400.0, 300.0],
            Pose::IDENTITY,
            Pose::at([0.0, 0.0, -3.0]),
        );
        assert_eq!(part, [0.0, 0.0, 400.0, 300.0]);
        assert_eq!(
            space.panels()[0].pose,
            carried,
            "the modifier's pose yields"
        );
        space.place(
            panel,
            [400.0, 320.0],
            Pose::IDENTITY,
            Pose::at([0.0, 0.0, -3.0]),
        );
        assert_eq!(space.panels()[0].pose, carried);
        assert_eq!(space.panels()[0].atlas, [0.0, 0.0, 400.0, 320.0]);
    });
}

// Two panels of 400 by 300 dp and 300 by 200 dp in the atlas, the first of which the user may
// resize.
fn resizable_pair(space: &Space) -> (Panel, Panel) {
    let (first, second) = (space.new_panel(), space.new_panel());
    space.place(first, [400.0, 300.0], Pose::IDENTITY, Pose::IDENTITY);
    space.place(second, [300.0, 200.0], Pose::IDENTITY, Pose::IDENTITY);
    let policy = ResizePolicy::custom(|_| {});
    space.set_resizable(first.id, Some((ResizeLimits::new(), &policy)));
    (first, second)
}

fn resize_step(panel: u64, event_type: SpatialResizeEventType, size: [f32; 2]) -> PanelResize {
    PanelResize {
        panel,
        event_type,
        size,
    }
}

#[test]
fn a_panel_in_a_resize_holds_its_room_in_steps_and_settles_at_the_release() {
    with_space(|space| {
        let (first, second) = resizable_pair(&space);
        let step = |event_type, size| resize_step(first.id, event_type, size);
        space.resize(&step(SpatialResizeEventType::Start, [400.0, 300.0]));
        assert_eq!(part(&space, second.id)[1], 308.0, "the grab stacks nothing");

        // The app applies a size that grows by a dp in each frame, as a drag does
        let mut tops = vec![part(&space, second.id)[1]];
        let mut atlases = vec![space.atlas_size()];
        for grown in 1..=200_u16 {
            let size = [400.0 + f32::from(grown), 300.0 + f32::from(grown)];
            space.resize(&step(SpatialResizeEventType::Resizing, size));
            space.place(first, size, Pose::IDENTITY, Pose::IDENTITY);
            assert_eq!(part(&space, first.id)[2..], size, "the panel has its size");
            let (top, atlas) = (part(&space, second.id)[1], space.atlas_size());
            if tops.last() != Some(&top) {
                tops.push(top);
            }
            if atlases.last() != Some(&atlas) {
                atlases.push(atlas);
            }
        }
        // 200 dp of growth in each direction stack the panels and make the atlas anew at the two
        // steps of the height and the two of the width, not at each of the 200 frames
        assert_eq!(tops, [308.0, 392.0, 520.0]);
        assert_eq!(atlases.len(), 4, "{atlases:?}");
        assert_eq!(space.panels()[0].room, [640.0, 512.0]);

        space.resize(&step(SpatialResizeEventType::End, [600.0, 500.0]));
        assert_eq!(space.panels()[0].room, [600.0, 500.0], "the room settles");
        assert_eq!(part(&space, second.id)[1], 508.0);
        assert_eq!(space.atlas_size(), [600.0, 708.0]);
    });
}

#[test]
fn a_panel_that_shrinks_in_a_resize_keeps_its_room_until_the_release() {
    with_space(|space| {
        let (first, second) = resizable_pair(&space);
        let step = |event_type, size| resize_step(first.id, event_type, size);
        space.resize(&step(SpatialResizeEventType::Start, [400.0, 300.0]));
        space.place(first, [200.0, 100.0], Pose::IDENTITY, Pose::IDENTITY);
        assert_eq!(part(&space, second.id)[1], 308.0);
        assert_eq!(space.panels()[0].room, [400.0, 300.0]);
        // The release settles even when the content places the panel at the same size again
        space.resize(&step(SpatialResizeEventType::End, [200.0, 100.0]));
        assert_eq!(part(&space, second.id)[1], 108.0);
        assert_eq!(space.panels()[0].room, [200.0, 100.0]);
    });
}

#[test]
fn a_panel_that_stops_being_resizable_in_a_resize_settles() {
    with_space(|space| {
        let (first, second) = resizable_pair(&space);
        space.resize(&resize_step(
            first.id,
            SpatialResizeEventType::Start,
            [400.0, 300.0],
        ));
        space.place(first, [500.0, 400.0], Pose::IDENTITY, Pose::IDENTITY);
        assert_eq!(space.panels()[0].room, [512.0, 512.0]);
        space.set_resizable(first.id, None);
        assert_eq!(space.panels()[0].room, [500.0, 400.0]);
        assert_eq!(part(&space, second.id)[1], 408.0);
        assert_eq!(space.panels()[0].resize, None);
    });
}

// A panel whose size is a state that its resize policy sets, as an app feeds it.
#[composable]
fn StatefulPanel(size: MutableState<[f32; 2]>) {
    let [width, height] = size.get();
    let policy = ResizePolicy::custom(move |event| size.set(event.size));
    SpatialPanel(
        SubspaceModifier::new()
            .size(width, height)
            .resizable(policy),
        || {},
    );
}

#[test]
fn an_app_that_applies_each_size_resizes_its_panel_through_the_composition() {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let mut robot = create_headless_robot_test(500, 1000, move || {
        let space = remember(Space::new).with(Clone::clone);
        let size = rememberMutableStateOf(|| [400.0, 300.0]);
        keep.replace(Some(space.clone()));
        CompositionLocalProvider([local_space().provides(Some(space))], move || {
            StatefulPanel(size);
        });
    });
    robot.wait_for_idle();
    let space = held.take().expect("the content composed");
    let id = space.panels()[0].id;
    let step = |event_type, size| resize_step(id, event_type, size);

    space.resize(&step(SpatialResizeEventType::Start, [400.0, 300.0]));
    for grown in 1..=30_u16 {
        let size = [
            400.0 + 10.0 * f32::from(grown),
            300.0 + 10.0 * f32::from(grown),
        ];
        space.resize(&step(SpatialResizeEventType::Resizing, size));
        robot.wait_for_idle();
        let panel = space.panels()[0];
        assert_eq!(
            panel.size(),
            size,
            "the policy set the state that sizes the panel"
        );
        assert!(panel.room[0] >= size[0] && panel.room[1] >= size[1]);
    }
    assert_eq!(
        space.panels()[0].room,
        [768.0, 640.0],
        "the room grew in steps"
    );

    space.resize(&step(SpatialResizeEventType::End, [700.0, 600.0]));
    robot.wait_for_idle();
    let panel = space.panels()[0];
    assert_eq!(panel.size(), [700.0, 600.0]);
    assert_eq!(
        panel.room,
        [700.0, 600.0],
        "the room settles at the release"
    );
}

#[test]
fn a_volume_takes_no_room_in_the_atlas() {
    with_space(|space| {
        let (first, volume, second) = (space.new_panel(), space.new_panel(), space.new_panel());
        space.place(first, [800.0, 500.0], Pose::IDENTITY, Pose::IDENTITY);
        space.place_volume(
            volume,
            [300.0, 300.0, 300.0],
            Pose::IDENTITY,
            Pose::at([0.0, 0.0, -1.0]),
        );
        assert_eq!(
            space.place(second, [300.0, 200.0], Pose::IDENTITY, Pose::IDENTITY),
            [0.0, 508.0, 300.0, 200.0]
        );
        assert_eq!(space.atlas_size(), [800.0, 708.0]);
        assert_eq!(space.volume_count(), 1);
    });
}

#[test]
fn a_volume_follows_the_depth_and_the_size_that_its_modifier_gives() {
    with_space(|space| {
        let volume = space.new_panel();
        let place = |depth| {
            space.place_volume(
                volume,
                [300.0, 200.0, depth],
                Pose::IDENTITY,
                Pose::IDENTITY,
            );
            space.panels()[0]
        };
        assert_eq!(place(300.0).kind, PanelKind::Volume { depth: 300.0 });
        let thinner = place(100.0);
        assert_eq!(thinner.kind, PanelKind::Volume { depth: 100.0 });
        assert_eq!(thinner.size(), [300.0, 200.0]);
        // The face that the handles stand at lies half the depth toward the viewer
        assert!((thinner.face().position[2] - 0.05).abs() < 1e-6);
    });
}

#[test]
fn a_volume_that_leaves_leaves_the_scene_too() {
    with_space(|space| {
        let (kept, left) = (space.new_panel(), space.new_panel());
        for panel in [kept, left] {
            space.place_volume(panel, [100.0; 3], Pose::IDENTITY, Pose::IDENTITY);
            space.publish_volume(VolumeContent {
                id: panel.id,
                pose: panel.volume_pose,
                light: mutableStateOf(None),
                compose: Rc::default(),
            });
        }
        assert_eq!(space.volumes().contents().get().len(), 2);
        space.remove(left.id);
        let remaining = space.volumes().contents().get();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, kept.id);
        assert_eq!(space.volume_count(), 1);
    });
}

#[test]
fn a_volume_that_a_following_subspace_moves_moves_in_the_scene_too() {
    with_space(|space| {
        let volume = space.new_panel();
        space.place_volume(
            volume,
            [100.0; 3],
            Pose::IDENTITY,
            Pose::at([0.0, 0.0, -1.0]),
        );
        assert_eq!(volume.volume_pose.get(), Pose::at([0.0, 0.0, -1.0]));
        // A new place from the modifier reaches the scene's state at once
        space.place_volume(
            volume,
            [100.0; 3],
            Pose::IDENTITY,
            Pose::at([0.5, 0.0, -1.0]),
        );
        assert_eq!(volume.volume_pose.get(), Pose::at([0.5, 0.0, -1.0]));
    });
}
