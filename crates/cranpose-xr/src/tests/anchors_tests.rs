use std::{cell::RefCell, rc::Rc};

use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::create_headless_robot_test;

use crate::{
    AnchorCapabilities, AnchorPersistence, AnchorResult, Anchors, Plane, PlaneLabel, PlaneState,
    PlaneType, Planes, Pose, TrackingAvailability, TrackingState, Uuid, local_anchors,
    local_planes, plane::PlaneKey, spatial_store::SpatialStore,
};

fn uuid(last: u8) -> Uuid {
    let mut bytes = [0_u8; 16];
    bytes[15] = last;
    Uuid::from_bytes(bytes)
}

#[test]
fn content_outside_a_headset_finds_no_anchors_and_no_planes() {
    let found = Rc::new(RefCell::new(None));
    let seen = Rc::clone(&found);
    let _robot = create_headless_robot_test(100, 100, move || {
        seen.replace(Some((
            local_anchors().current().is_some(),
            local_planes().current().is_some(),
        )));
    });
    assert_eq!(*found.borrow(), Some((false, false)));
}

// What an app does with the handles that the headset provides, from one composition.
#[test]
fn an_app_creates_keeps_and_loads_an_anchor_and_lists_the_planes_through_the_locals() {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let robot = create_headless_robot_test(100, 100, move || {
        let store = remember(|| {
            SpatialStore::new(
                AnchorCapabilities {
                    anchors: TrackingAvailability::Active,
                    persistence: TrackingAvailability::Active,
                    max_anchors: Some(4),
                },
                TrackingAvailability::Active,
            )
        })
        .with(Clone::clone);
        keep.replace(Some(Rc::clone(&store)));
        let (anchors, planes) = (Anchors::new(Rc::clone(&store)), Planes::new(store));
        CompositionLocalProvider(
            [
                local_anchors().provides(Some(anchors)),
                local_planes().provides(Some(planes)),
            ],
            || {},
        );
    });
    let store = held.take().expect("the content composed");
    let anchors = Anchors::new(Rc::clone(&store));
    let planes = Planes::new(Rc::clone(&store));
    assert_eq!(anchors.capabilities().max_anchors, Some(4));
    assert_eq!(planes.availability(), TrackingAvailability::Active);

    let anchor = anchors
        .create(Pose::at([0.0, 1.0, -2.0]))
        .into_anchor()
        .expect("anchors are available");
    anchor.persist();
    store.finish_persist(&anchor, Some(uuid(5)));
    assert_eq!(anchor.persistence(), AnchorPersistence::Persisted(uuid(5)));
    assert_eq!(anchors.persisted_uuids(), vec![uuid(5)]);

    let loaded = anchors.load(uuid(5));
    assert!(matches!(loaded, AnchorResult::Success(_)));
    assert_eq!(anchors.load(uuid(6)), AnchorResult::InvalidUuid);
    anchors.unpersist(uuid(5));
    assert!(anchors.persisted_uuids().is_empty());

    let state = PlaneState {
        center_pose: Pose::at([0.0, 0.0, -3.0]),
        extents: [4.0, 2.5],
        label: PlaneLabel::Wall,
        tracking_state: TrackingState::Tracking,
    };
    store.add_plane(Plane::new(
        PlaneKey(1),
        PlaneType::Vertical,
        state,
        store.weak(),
    ));
    let listed = planes.all();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].plane_type(), PlaneType::Vertical);
    assert_eq!(listed[0].state().label, PlaneLabel::Wall);
    assert_eq!(listed[0].with_vertices(<[_]>::len), 0);
    assert_eq!(anchors, anchors.clone());
    drop(robot);
}
