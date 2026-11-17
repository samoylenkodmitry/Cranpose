use super::*;
use crate::pose::turn_degrees;

const PANEL: Pose = Pose::at([0.0, 0.0, -1.0]);
const SIZE: [f32; 2] = [0.8, 0.6];

fn hit(ray: Ray) -> Option<(f32, f32, f32)> {
    ray.hit_panel(&PANEL, SIZE)
        .map(|hit| (hit.distance, hit.u, hit.v))
}

#[test]
fn a_ray_ahead_hits_the_panel_center() {
    let ray = Ray::of_aim(&Pose::IDENTITY);
    let (distance, u, v) = hit(ray).expect("the ray ahead hits");
    assert!((distance - 1.0).abs() < 1e-5);
    assert!((u - 0.5).abs() < 1e-5 && (v - 0.5).abs() < 1e-5);
}

#[test]
fn a_ray_to_the_upper_left_hits_near_the_top_left_corner() {
    let ray = Ray {
        origin: [-0.35, 0.25, 0.0],
        direction: [0.0, 0.0, -1.0],
    };
    let (_, u, v) = hit(ray).expect("the ray hits");
    assert!((u - 0.0625).abs() < 1e-5, "u {u}");
    assert!((v - 0.083_333).abs() < 1e-4, "v {v}");
}

#[test]
fn a_ray_away_from_the_panel_misses() {
    let backwards = Ray {
        origin: [0.0; 3],
        direction: [0.0, 0.0, 1.0],
    };
    let beside = Ray {
        origin: [0.5, 0.0, 0.0],
        direction: [0.0, 0.0, -1.0],
    };
    let along = Ray {
        origin: [0.0, 0.0, -1.0],
        direction: [1.0, 0.0, 0.0],
    };
    assert_eq!(hit(backwards), None);
    assert_eq!(hit(beside), None);
    assert_eq!(hit(along), None);
}

#[test]
fn a_turned_panel_takes_the_hit_in_its_own_axes() {
    // The panel stands to the left, turned to face the viewer at the origin
    let panel = Pose {
        position: [-1.0, 0.0, 0.0],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    let aim = Pose {
        position: [0.0, 0.0, 0.0],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    let hit = Ray::of_aim(&aim)
        .hit_panel(&panel, SIZE)
        .expect("the ray hits");
    assert!((hit.u - 0.5).abs() < 1e-5 && (hit.v - 0.5).abs() < 1e-5);
    let right_of_center = Ray {
        origin: [0.0, 0.0, -0.2],
        direction: [-1.0, 0.0, 0.0],
    };
    let hit = right_of_center
        .hit_panel(&panel, SIZE)
        .expect("the ray hits");
    assert!((hit.u - 0.75).abs() < 1e-5, "u {}", hit.u);
}
