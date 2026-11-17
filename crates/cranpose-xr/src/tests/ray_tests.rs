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

// A panel 1.2 m wide along its arc and 0.4 m tall, a meter ahead, on a cylinder of one meter
// around the origin.
const CURVED: [f32; 2] = [1.2, 0.4];

fn curved_hit(ray: Ray) -> Option<PanelHit> {
    ray.hit_cylinder(&PANEL, CURVED, 1.0)
}

fn toward(degrees: f32) -> Ray {
    let (sin, cos) = degrees.to_radians().sin_cos();
    Ray {
        origin: [0.0; 3],
        direction: [sin, 0.0, -cos],
    }
}

#[test]
fn a_ray_from_the_axis_meets_the_curve_at_the_radius_and_the_arc_length() {
    let hit = curved_hit(toward(20.0)).expect("the ray hits");
    assert!(
        (hit.distance - 1.0).abs() < 1e-5,
        "distance {}",
        hit.distance
    );
    // 20 degrees of a meter-wide circle are 0.349 m of arc
    assert!(
        (hit.u - (0.5 + 0.349_066 / 1.2)).abs() < 1e-4,
        "u {}",
        hit.u
    );
    let flat = toward(20.0)
        .hit_panel(&PANEL, CURVED)
        .expect("the ray hits a flat panel");
    assert!(
        flat.u - hit.u > 0.01,
        "a flat panel puts the hit farther out"
    );
}

#[test]
fn a_ray_from_beside_the_axis_meets_the_curve_in_front_of_it() {
    let ray = Ray {
        origin: [0.3, 0.1, 0.0],
        direction: [0.0, 0.0, -1.0],
    };
    let hit = curved_hit(ray).expect("the ray hits");
    // The cylinder is at z = 1 - sqrt(1 - 0.3^2), so the hit is that far down the ray
    let expected = 1.0 - (1.0_f32 - 0.09).sqrt();
    assert!((hit.distance - (1.0 - expected)).abs() < 1e-4);
    assert!((hit.v - 0.25).abs() < 1e-4, "v {}", hit.v);
}

#[test]
fn a_ray_past_the_arc_or_the_height_misses() {
    // The arc ends at 0.6 rad, which is 34.4 degrees
    assert!(curved_hit(toward(33.0)).is_some());
    assert_eq!(curved_hit(toward(36.0)), None);
    let above = Ray {
        origin: [0.0, 0.3, 0.0],
        direction: [0.0, 0.0, -1.0],
    };
    assert_eq!(curved_hit(above), None);
    let backwards = Ray {
        origin: [0.0; 3],
        direction: [0.0, 0.0, 1.0],
    };
    assert_eq!(curved_hit(backwards), None);
}

#[test]
fn a_curve_without_a_radius_is_flat() {
    let ray = toward(20.0);
    for radius in [0.0, -1.0, f32::INFINITY] {
        assert_eq!(
            ray.hit_cylinder(&PANEL, CURVED, radius),
            ray.hit_panel(&PANEL, CURVED)
        );
    }
}
