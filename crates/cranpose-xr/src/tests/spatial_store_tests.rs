use std::{cell::RefCell, rc::Rc};

use cranpose_core::remember;
use cranpose_testing::create_headless_robot_test;

use super::*;
use crate::{Anchors, PlaneLabel, PlaneState, PlaneType, anchor::AnchorOrigin};

fn ready() -> AnchorCapabilities {
    AnchorCapabilities {
        anchors: TrackingAvailability::Active,
        persistence: TrackingAvailability::Active,
        max_anchors: None,
    }
}

fn uuid(last: u8) -> Uuid {
    let mut bytes = [0_u8; 16];
    bytes[15] = last;
    Uuid::from_bytes(bytes)
}

fn at(x: f32) -> Pose {
    Pose::at([x, 0.0, -1.0])
}

// Runs `check` with a store that a runtime made, as a composition makes it.
fn with_store(capabilities: AnchorCapabilities, check: impl FnOnce(&Rc<SpatialStore>)) {
    cranpose_testing::run_test_composition(|_| {
        check(&SpatialStore::new(
            capabilities,
            TrackingAvailability::Active,
        ));
    });
}

fn made(result: AnchorResult) -> Anchor {
    result.into_anchor().expect("the store made the anchor")
}

#[test]
fn an_app_that_did_not_ask_for_anchors_gets_none() {
    for availability in [
        TrackingAvailability::Unavailable,
        TrackingAvailability::Offered,
        TrackingAvailability::AwaitingPermission,
        TrackingAvailability::PermissionRefused,
    ] {
        let capabilities = AnchorCapabilities {
            anchors: availability,
            ..ready()
        };
        with_store(capabilities, |store| {
            assert_eq!(store.create_anchor(at(0.0)), AnchorResult::Unavailable);
            assert_eq!(store.anchor_count(), 0);
        });
    }
}

#[test]
fn a_new_anchor_stands_at_its_pose_and_waits_for_the_runtime_to_hold_it() {
    with_store(ready(), |store| {
        let anchor = made(store.create_anchor(at(2.0)));
        let state = anchor.state();
        assert_eq!(state.pose, at(2.0));
        assert_eq!(state.tracking_state, TrackingState::Paused);
        assert_eq!(anchor.persistence(), AnchorPersistence::NotPersisted);
        assert_eq!(anchor.uuid(), None);
        assert!(anchor.is_located());
        assert_eq!(anchor.origin(), AnchorOrigin::Space);
        assert_eq!(store.anchor_count(), 1);
    });
}

#[test]
fn the_runtimes_frames_move_the_anchor_and_a_change_of_its_tracking_state_counts() {
    with_store(ready(), |store| {
        let anchor = made(store.create_anchor(at(0.0)));
        store.publish_anchor(&anchor, Some(at(0.01)), TrackingState::Tracking);
        assert_eq!(anchor.state().pose, at(0.01));
        assert_eq!(anchor.state().tracking_state, TrackingState::Tracking);
        // A frame that gives no pose keeps the last one and pauses the anchor
        store.publish_anchor(&anchor, None, TrackingState::Paused);
        assert_eq!(anchor.state().pose, at(0.01));
        assert_eq!(anchor.state().tracking_state, TrackingState::Paused);
    });
}

#[test]
fn a_head_that_is_not_tracked_leaves_no_place_to_make_an_anchor() {
    with_store(ready(), |store| {
        store.set_head_tracked(false);
        assert_eq!(
            store.create_anchor(at(0.0)),
            AnchorResult::TrackingUnavailable
        );
        store.set_head_tracked(true);
        assert!(store.create_anchor(at(0.0)).anchor().is_some());
    });
}

#[test]
fn a_runtime_that_allows_two_anchors_refuses_the_third_until_one_ends() {
    let capabilities = AnchorCapabilities {
        max_anchors: Some(2),
        ..ready()
    };
    with_store(capabilities, |store| {
        let first = made(store.create_anchor(at(0.0)));
        let _second = made(store.create_anchor(at(1.0)));
        assert_eq!(
            store.create_anchor(at(2.0)),
            AnchorResult::ResourcesExhausted
        );
        // A detached anchor no longer counts, before the next frame's sweep lets it go
        first.detach();
        assert!(store.create_anchor(at(2.0)).anchor().is_some());
    });
}

#[test]
fn an_anchor_goes_from_not_persisted_through_persisting_to_persisted_with_its_identifier() {
    with_store(ready(), |store| {
        let anchor = made(store.create_anchor(at(0.0)));
        assert!(!anchor.needs_persist());
        anchor.persist();
        assert_eq!(anchor.persistence(), AnchorPersistence::Persisting);
        assert!(anchor.needs_persist(), "the backend passes the ask on once");
        anchor.mark_persist_sent();
        assert!(!anchor.needs_persist());
        // A second ask while the runtime works changes nothing
        anchor.persist();
        assert!(!anchor.needs_persist());
        store.finish_persist(&anchor, Some(uuid(7)));
        assert_eq!(anchor.persistence(), AnchorPersistence::Persisted(uuid(7)));
        assert_eq!(anchor.uuid(), Some(uuid(7)));
        assert_eq!(store.persisted_uuids(), vec![uuid(7)]);
        anchor.persist();
        assert_eq!(anchor.persistence(), AnchorPersistence::Persisted(uuid(7)));
    });
}

#[test]
fn a_refused_save_fails_and_a_later_ask_starts_again() {
    with_store(ready(), |store| {
        let anchor = made(store.create_anchor(at(0.0)));
        anchor.persist();
        anchor.mark_persist_sent();
        store.finish_persist(&anchor, None);
        assert_eq!(anchor.persistence(), AnchorPersistence::Failed);
        assert!(store.persisted_uuids().is_empty());
        anchor.persist();
        assert_eq!(anchor.persistence(), AnchorPersistence::Persisting);
    });
}

#[test]
fn an_app_with_no_storage_fails_at_once_and_loads_nothing() {
    let capabilities = AnchorCapabilities {
        persistence: TrackingAvailability::AwaitingPermission,
        ..ready()
    };
    with_store(capabilities, |store| {
        let anchor = made(store.create_anchor(at(0.0)));
        anchor.persist();
        assert_eq!(anchor.persistence(), AnchorPersistence::Failed);
        assert!(!anchor.needs_persist());
        assert_eq!(store.load(uuid(1)), AnchorResult::Unavailable);
    });
}

#[test]
fn a_persisted_anchor_loads_back_unplaced_until_the_runtime_locates_it() {
    with_store(ready(), |store| {
        assert_eq!(store.load(uuid(3)), AnchorResult::InvalidUuid);
        store.set_persisted(&[uuid(3), uuid(4)]);
        let anchor = made(store.load(uuid(3)));
        assert_eq!(anchor.origin(), AnchorOrigin::Persisted(uuid(3)));
        assert_eq!(anchor.persistence(), AnchorPersistence::Persisted(uuid(3)));
        assert!(!anchor.is_located());
        assert_eq!(anchor.state().tracking_state, TrackingState::Paused);
        store.publish_anchor(&anchor, Some(at(1.5)), TrackingState::Tracking);
        assert!(anchor.is_located());
        assert_eq!(anchor.state().pose, at(1.5));
    });
}

#[test]
fn unpersisting_removes_the_identifier_and_tells_the_backend_once() {
    with_store(ready(), |store| {
        store.set_persisted(&[uuid(1), uuid(2)]);
        store.unpersist(uuid(1));
        store.unpersist(uuid(9));
        assert_eq!(store.persisted_uuids(), vec![uuid(2)]);
        let mut asked = Vec::new();
        store.take_unpersists(&mut asked);
        assert_eq!(asked, vec![uuid(1)]);
        store.take_unpersists(&mut asked);
        assert_eq!(asked, vec![uuid(1)]);
    });
}

#[test]
fn a_detached_anchor_stops_and_the_sweep_lets_it_go_while_the_app_holds_another() {
    with_store(ready(), |store| {
        let gone = made(store.create_anchor(at(0.0)));
        let kept = made(store.create_anchor(at(1.0)));
        gone.detach();
        assert_eq!(gone.state().tracking_state, TrackingState::Stopped);
        // The runtime's frames no longer move a detached anchor
        store.publish_anchor(&gone, Some(at(5.0)), TrackingState::Tracking);
        assert_eq!(gone.state().pose, at(0.0));
        assert_eq!(gone.state().tracking_state, TrackingState::Stopped);
        gone.persist();
        assert_eq!(gone.persistence(), AnchorPersistence::NotPersisted);
        store.sweep();
        assert_eq!(store.anchor_count(), 1);
        assert_eq!(store.anchor_at(0), Some(kept));
    });
}

#[test]
fn an_anchor_that_no_handle_holds_ends_with_the_next_sweep() {
    with_store(ready(), |store| {
        let anchor = made(store.create_anchor(at(0.0)));
        let held = anchor.clone();
        drop(anchor);
        store.sweep();
        assert_eq!(store.anchor_count(), 1, "a clone still holds it");
        drop(held);
        store.sweep();
        assert_eq!(store.anchor_count(), 0);
    });
}

#[test]
fn what_the_backend_attaches_to_an_anchor_ends_with_the_anchor() {
    struct Resource(Rc<RefCell<u32>>);
    impl Drop for Resource {
        fn drop(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }
    with_store(ready(), |store| {
        let ended = Rc::new(RefCell::new(0));
        let anchor = made(store.create_anchor(at(0.0)));
        anchor.attach(Resource(Rc::clone(&ended)));
        assert!(anchor.is_attached());
        assert_eq!(anchor.with_resource(|_: &Resource| 1), Some(1));
        drop(anchor);
        store.sweep();
        assert_eq!(*ended.borrow(), 1);
        // A detached anchor gives its resource back to the runtime at once, though the app keeps a
        // handle
        let detached = made(store.create_anchor(at(1.0)));
        detached.attach(Resource(Rc::clone(&ended)));
        detached.detach();
        assert_eq!(*ended.borrow(), 2);
        assert!(!detached.is_attached());
    });
}

fn plane(store: &Rc<SpatialStore>, key: u128, plane_type: PlaneType, label: PlaneLabel) -> Plane {
    let state = PlaneState {
        center_pose: Pose::at([0.0, -0.7, -1.0]),
        extents: [1.0, 1.0],
        label,
        tracking_state: TrackingState::Tracking,
    };
    let plane = Plane::new(PlaneKey(key), plane_type, state, store.weak());
    store.add_plane(plane.clone());
    plane
}

#[test]
fn the_planes_that_the_app_lists_are_the_tracked_ones_that_no_other_plane_subsumes() {
    with_store(ready(), |store| {
        let table = plane(
            store,
            1,
            PlaneType::HorizontalUpwardFacing,
            PlaneLabel::Table,
        );
        let wall = plane(store, 2, PlaneType::Vertical, PlaneLabel::Wall);
        let lost = plane(store, 3, PlaneType::Vertical, PlaneLabel::Wall);
        assert_eq!(
            store.tracked_planes(),
            vec![table.clone(), wall.clone(), lost.clone()]
        );
        lost.publish(PlaneState {
            tracking_state: TrackingState::Paused,
            ..lost.current()
        });
        wall.set_subsumed_by(Some(PlaneKey(1)));
        // The runtime may turn a plane that it first took for a wall into a table
        lost.set_plane_type(PlaneType::HorizontalUpwardFacing);
        assert_eq!(lost.plane_type(), PlaneType::HorizontalUpwardFacing);
        assert_eq!(store.tracked_planes(), vec![table.clone()]);
        assert_eq!(wall.subsumed_by(), Some(table));
        assert!(store.retain_planes(|plane| plane.key() != PlaneKey(3)));
        assert_eq!(store.plane_count(), 2);
    });
}

#[test]
fn a_plane_creates_an_anchor_on_itself_and_the_runtime_is_told_which() {
    with_store(ready(), |store| {
        let table = plane(
            store,
            5,
            PlaneType::HorizontalUpwardFacing,
            PlaneLabel::Table,
        );
        let anchor = made(table.create_anchor(at(0.2)));
        assert_eq!(anchor.origin(), AnchorOrigin::Plane(PlaneKey(5)));
        assert_eq!(anchor.state().pose, at(0.2));
    });
}

#[test]
fn a_plane_that_outlives_its_store_makes_no_anchor() {
    let orphan = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&orphan);
    with_store(ready(), move |store| {
        keep.replace(Some(plane(store, 1, PlaneType::Vertical, PlaneLabel::Wall)));
    });
    let plane = orphan.take().expect("the plane exists");
    assert_eq!(plane.create_anchor(at(0.0)), AnchorResult::Unavailable);
    assert_eq!(plane.subsumed_by(), None);
}

// A composition reads the anchor's persistence and keeps what it saw.
#[test]
fn a_composition_that_reads_the_persistence_composes_again_when_it_changes() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let held = Rc::new(RefCell::new(None));
    let (reader, keep) = (Rc::clone(&seen), Rc::clone(&held));
    let mut robot = create_headless_robot_test(100, 100, move || {
        let store = remember(|| SpatialStore::new(ready(), TrackingAvailability::Active))
            .with(Clone::clone);
        let anchors = Anchors::new(Rc::clone(&store));
        let anchor = remember({
            let anchors = anchors.clone();
            move || made(anchors.create(at(0.0)))
        })
        .with(Clone::clone);
        reader
            .borrow_mut()
            .push((anchor.persistence(), anchors.persisted_uuids().len()));
        keep.replace(Some((store, anchor)));
    });
    robot.wait_for_idle();
    let (store, anchor) = held.take().expect("the content composed");
    anchor.persist();
    robot.wait_for_idle();
    store.finish_persist(&anchor, Some(uuid(9)));
    robot.wait_for_idle();
    assert_eq!(
        *seen.borrow(),
        vec![
            (AnchorPersistence::NotPersisted, 0),
            (AnchorPersistence::Persisting, 0),
            (AnchorPersistence::Persisted(uuid(9)), 1),
        ]
    );
    // A pose that moves in every frame composes nothing
    let composed = seen.borrow().len();
    for step in 0..20 {
        store.publish_anchor(
            &anchor,
            Some(at(step as f32 * 0.001)),
            TrackingState::Paused,
        );
        robot.wait_for_idle();
    }
    assert_eq!(seen.borrow().len(), composed);
}
