use std::{cell::RefCell, rc::Rc};

use cranpose::remember;
use cranpose_core::CompositionLocalProvider;
use cranpose_testing::{
    create_headless_robot_test,
    robot::{RobotTestRule, TestRenderer},
};

use super::*;
use crate::{
    SpatialPanel, SubspaceModifier,
    space::{Space, Surface, local_space},
};

// The space of a headset that shows `content`, and the composition that keeps its panels.
fn headset(
    curved: bool,
    content: impl FnMut() + Clone + 'static,
) -> (Space, RobotTestRule<TestRenderer>) {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let robot = create_headless_robot_test(500, 1000, move || {
        let space = remember(Space::new).with(Clone::clone);
        space.set_curves_shown(curved);
        keep.replace(Some(space.clone()));
        CompositionLocalProvider([local_space().provides(Some(space))], content.clone());
    });
    (held.take().expect("the content composed"), robot)
}

// Two panels of 400 dp at the row's middle and 45 degrees along a meter-radius curve, and a
// panel outside any row.
fn row(radius: f32) -> impl FnMut() + Clone + 'static {
    move || {
        SpatialCurvedRow(SubspaceModifier::new(), radius, || {
            SpatialPanel(SubspaceModifier::new().size(400.0, 300.0), || {});
            SpatialPanel(
                SubspaceModifier::new().size(400.0, 300.0).offset(
                    1000.0 * std::f32::consts::FRAC_PI_4,
                    0.0,
                    0.0,
                ),
                || {},
            );
        });
        SpatialPanel(SubspaceModifier::new().size(400.0, 300.0), || {});
    }
}

#[test]
fn the_panels_of_a_row_stand_on_one_cylinder_around_the_viewer() {
    let (space, _robot) = headset(true, row(1000.0));
    let panels = space.panels();
    assert_eq!(panels.len(), 3);
    for slot in &panels[..2] {
        assert_eq!(slot.surface, Surface::Curved { radius: 1.0 });
        // The cylinder's axis is the viewer's upright line through the origin
        let along = slot.pose.rotate([0.0, 0.0, 1.0]);
        let axis = [0, 1, 2].map(|i| slot.pose.position[i] + along[i]);
        assert!(axis.iter().all(|v| v.abs() < 1e-5), "axis {axis:?}");
    }
    assert_eq!(panels[0].pose.position, [0.0, 0.0, -1.0]);
    let [x, _, z] = panels[1].pose.position;
    let side = std::f32::consts::FRAC_1_SQRT_2;
    assert!((x - side).abs() < 1e-3 && (z + side).abs() < 1e-3);
    assert_eq!(panels[2].surface, Surface::Flat, "outside the row");
}

#[test]
fn a_runtime_without_cylinder_layers_shows_the_row_flat() {
    let (space, _robot) = headset(false, row(1000.0));
    assert!(
        space
            .panels()
            .iter()
            .all(|slot| slot.surface == Surface::Flat)
    );
    space.set_curves_shown(true);
    assert_eq!(space.panels()[0].surface, Surface::Curved { radius: 1.0 });
}

#[test]
fn a_row_without_a_radius_is_straight() {
    for radius in [0.0, -5.0, f32::INFINITY] {
        let (space, _robot) = headset(true, row(radius));
        let panels = space.panels();
        assert!(panels.iter().all(|slot| slot.surface == Surface::Flat));
        let [x, _, z] = panels[1].pose.position;
        assert!(
            (x - std::f32::consts::FRAC_PI_4).abs() < 1e-3 && (z + 1.0).abs() < 1e-5,
            "{x} {z}"
        );
    }
}
