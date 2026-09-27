use super::{Arrangement, LinearArrangement};

fn arranged(arrangement: LinearArrangement, density: f32, total: f32, sizes: &[f32]) -> Vec<f32> {
    let mut positions = vec![0.0; sizes.len()];
    arrangement.arrange(density, total, sizes, &mut positions);
    positions
}

#[test]
fn space_evenly_rounds_each_position_to_a_device_pixel() {
    let sizes = [10.0, 10.0, 10.0];
    assert_eq!(
        arranged(LinearArrangement::SpaceEvenly, 1.0, 100.0, &sizes),
        [18.0, 45.0, 73.0]
    );
    assert_eq!(
        arranged(LinearArrangement::SpaceEvenly, 2.0, 100.0, &sizes),
        [17.5, 45.0, 72.5]
    );
}

#[test]
fn center_sends_a_half_pixel_up_as_kotlin_does() {
    assert_eq!(
        arranged(LinearArrangement::Center, 1.0, 101.0, &[10.0]),
        [46.0]
    );
    assert_eq!(
        arranged(LinearArrangement::Center, 1.0, 9.0, &[10.0]),
        [0.0]
    );
}

#[test]
fn space_between_puts_a_lone_child_at_the_start() {
    assert_eq!(
        arranged(LinearArrangement::SpaceBetween, 1.0, 100.0, &[10.0]),
        [0.0]
    );
    assert_eq!(
        arranged(LinearArrangement::SpaceBetween, 1.0, 100.0, &[10.0, 10.0]),
        [0.0, 90.0]
    );
}

#[test]
fn space_around_and_end_follow_the_remaining_space() {
    assert_eq!(
        arranged(LinearArrangement::SpaceAround, 1.0, 100.0, &[10.0, 10.0]),
        [20.0, 70.0]
    );
    assert_eq!(
        arranged(LinearArrangement::End, 1.0, 100.0, &[10.0, 10.0]),
        [80.0, 90.0]
    );
    assert_eq!(
        arranged(LinearArrangement::Start, 1.0, 100.0, &[10.0, 10.0]),
        [0.0, 10.0]
    );
}

#[test]
fn spaced_by_puts_a_device_pixel_spacing_between_children() {
    assert_eq!(
        arranged(LinearArrangement::spaced_by(5.0), 1.0, 40.0, &[10.0, 10.0]),
        [0.0, 15.0]
    );
    assert_eq!(
        arranged(
            LinearArrangement::spaced_by(12.4),
            1.0,
            100.0,
            &[10.0, 10.0]
        ),
        [0.0, 22.0]
    );
    assert_eq!(LinearArrangement::spaced_by(12.4).spacing(1.0), 12.0);
    assert_eq!(LinearArrangement::spaced_by(-3.0).spacing(1.0), 0.0);
    assert_eq!(LinearArrangement::Center.spacing(1.0), 0.0);
}

#[test]
fn spaced_by_keeps_every_child_inside_the_container() {
    assert_eq!(
        arranged(
            LinearArrangement::spaced_by(12.0),
            1.0,
            100.0,
            &[95.0, 10.0]
        ),
        [0.0, 90.0]
    );
    assert_eq!(
        arranged(LinearArrangement::spaced_by(12.0), 1.0, 100.0, &[95.0, 0.0]),
        [0.0, 100.0]
    );
}
