use super::*;
use crate::OutputLevels;

fn scene() -> Arc<SpatialScene> {
    Arc::new(SpatialScene::new())
}

#[test]
fn the_listener_starts_at_the_origin_and_follows_what_is_set() {
    let scene = scene();
    assert_eq!(scene.listener(), Some(AudioListener::AT_ORIGIN));
    let head = AudioListener {
        position: [0.5, 1.6, -2.0],
        orientation: [
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
        ],
    };
    scene.set_listener(head);
    assert_eq!(scene.listener(), Some(head));
}

#[test]
fn an_emitter_reads_back_its_place_and_curve_until_it_is_dropped() {
    let scene = scene();
    let curve = DistanceAttenuation::Natural { min_distance: 2.0 };
    let emitter = scene
        .add_emitter([1.0, 2.0, 3.0], curve)
        .expect("a free slot");
    let id = emitter.id();
    assert_eq!(
        scene.read_emitter(id),
        EmitterReading::Placed {
            position: [1.0, 2.0, 3.0],
            attenuation: curve
        }
    );
    emitter.place([4.0, 5.0, 6.0]);
    assert_eq!(
        scene.read_emitter(id),
        EmitterReading::Placed {
            position: [4.0, 5.0, 6.0],
            attenuation: curve
        }
    );
    drop(emitter);
    assert_eq!(scene.read_emitter(id), EmitterReading::Gone);
}

#[test]
fn a_slot_that_another_emitter_took_does_not_answer_to_the_old_name() {
    let scene = scene();
    let first = scene
        .add_emitter([1.0; 3], DistanceAttenuation::None)
        .expect("a free slot");
    let old = first.id();
    drop(first);
    let second = scene
        .add_emitter([2.0; 3], DistanceAttenuation::None)
        .expect("a free slot");
    assert_ne!(second.id(), old);
    assert_eq!(scene.read_emitter(old), EmitterReading::Gone);
    assert_eq!(
        scene.read_emitter(second.id()),
        EmitterReading::Placed {
            position: [2.0; 3],
            attenuation: DistanceAttenuation::None
        }
    );
}

#[test]
fn the_scene_holds_max_emitters_and_a_dropped_one_makes_room() {
    let scene = scene();
    let mut held: Vec<Emitter> = (0..MAX_EMITTERS)
        .map(|_| {
            scene
                .add_emitter([0.0; 3], DistanceAttenuation::Auto)
                .expect("a free slot")
        })
        .collect();
    assert!(
        scene
            .add_emitter([0.0; 3], DistanceAttenuation::Auto)
            .is_none()
    );
    held.pop();
    assert!(
        scene
            .add_emitter([0.0; 3], DistanceAttenuation::Auto)
            .is_some()
    );
}

#[test]
fn the_pause_and_the_tap_are_the_scene_s_own() {
    let scene = scene();
    assert!(!scene.is_paused());
    scene.set_paused(true);
    assert!(scene.is_paused());
    assert!(!scene.tap().is_enabled());
    assert_eq!(scene.tap().levels(), None);
    scene.tap().set_enabled(true);
    scene.tap().publish(0.25, 0.5);
    scene.tap().publish(0.1, 0.2);
    assert_eq!(
        scene.tap().levels(),
        Some(OutputLevels {
            window: 2,
            left: 0.1,
            right: 0.2
        })
    );
}
