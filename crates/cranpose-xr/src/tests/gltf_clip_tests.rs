// The keyframes of a clip: how a track runs between keys, and what a clip does to a node.

use super::*;

fn near<const N: usize>(a: [f32; N], b: [f32; N]) {
    assert!(
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4),
        "{a:?} is not {b:?}"
    );
}

fn about_y(degrees: f32) -> [f32; 4] {
    let half = degrees.to_radians() / 2.0;
    [0.0, half.sin(), 0.0, half.cos()]
}

#[test]
fn a_linear_track_mixes_the_keys_and_holds_the_first_and_last_outside_them() {
    let track = Track::new(
        vec![1.0, 3.0],
        vec![[0.0, 0.0, 0.0], [4.0, 8.0, 2.0]],
        Interpolation::Linear,
    )
    .expect("a track");
    near(track.sample(0.0, lerp), [0.0, 0.0, 0.0]);
    near(track.sample(1.0, lerp), [0.0, 0.0, 0.0]);
    near(track.sample(2.0, lerp), [2.0, 4.0, 1.0]);
    near(track.sample(2.5, lerp), [3.0, 6.0, 1.5]);
    near(track.sample(3.0, lerp), [4.0, 8.0, 2.0]);
    near(track.sample(7.0, lerp), [4.0, 8.0, 2.0]);
    assert_eq!(track.end(), 3.0);
}

#[test]
fn a_step_track_holds_a_value_until_the_next_key() {
    let track = Track::new(
        vec![0.0, 1.0, 2.0],
        vec![[1.0], [2.0], [3.0]],
        Interpolation::Step,
    )
    .expect("a track");
    near(track.sample(0.99, lerp), [1.0]);
    near(track.sample(1.0, lerp), [2.0]);
    near(track.sample(1.99, lerp), [2.0]);
}

#[test]
fn a_rotation_takes_the_shorter_arc_between_two_keys() {
    // The second key is the same turn as 90 degrees, written with the opposite sign: the long way
    // round would pass 225 degrees
    let flipped = about_y(90.0).map(|v| -v);
    let track = Track::new(
        vec![0.0, 1.0],
        vec![about_y(0.0), flipped],
        Interpolation::Linear,
    )
    .expect("a track");
    let middle = track.sample(0.5, slerp);
    let expected = about_y(45.0);
    // Either sign is the same turn
    let sign = if middle[3] * expected[3] < 0.0 {
        -1.0
    } else {
        1.0
    };
    near(middle.map(|v| v * sign), expected);
    let length: f32 = middle.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((length - 1.0).abs() < 1e-4, "the turn has length {length}");
}

#[test]
fn a_cubic_spline_reads_the_value_between_the_tangents_of_each_key() {
    // Each key holds its in-tangent, its value and its out-tangent. The values go from 0 to 1
    // over two seconds, and the first key leaves with a slope of 2.
    let track = Track::new(
        vec![0.0, 2.0],
        vec![[0.0], [0.0], [2.0], [0.0], [1.0], [0.0]],
        Interpolation::CubicSpline,
    )
    .expect("a track");
    near(track.sample(0.0, lerp), [0.0]);
    near(track.sample(2.0, lerp), [1.0]);
    // At the middle the Hermite weights are 0.5 for each value and 0.125 for the out-tangent,
    // which the span of two seconds scales: 0.5 * 1 + 0.125 * 2 * 2
    near(track.sample(1.0, lerp), [1.0]);
    // With no tangents the curve is the smoothstep, which is flat at the keys
    let smooth = Track::new(
        vec![0.0, 1.0],
        vec![[0.0], [0.0], [0.0], [0.0], [1.0], [0.0]],
        Interpolation::CubicSpline,
    )
    .expect("a track");
    near(smooth.sample(0.5, lerp), [0.5]);
    assert!(smooth.sample(0.1, lerp)[0] < 0.1, "the start is flat");
}

#[test]
fn a_track_that_is_not_a_track_is_refused() {
    let rises = |times: Vec<f32>, values: usize, interpolation| {
        Track::new(times, vec![[0.0]; values], interpolation)
    };
    assert!(rises(vec![], 0, Interpolation::Linear).is_err(), "no keys");
    assert!(
        rises(vec![0.0, 1.0], 1, Interpolation::Linear).is_err(),
        "a value for one key only"
    );
    assert!(
        rises(vec![0.0, 1.0], 2, Interpolation::CubicSpline).is_err(),
        "a cubic key has three values"
    );
    assert!(
        rises(vec![1.0, 1.0], 2, Interpolation::Linear).is_err(),
        "equal times"
    );
    assert!(
        rises(vec![2.0, 1.0], 2, Interpolation::Step).is_err(),
        "falling times"
    );
    assert!(rises(vec![0.0, 1.0], 2, Interpolation::Linear).is_ok());
}

#[test]
fn a_clip_sets_only_the_keyed_parts_of_its_nodes_and_lasts_to_its_latest_key() {
    let rotation = Track::new(
        vec![0.0, 4.0],
        vec![about_y(0.0), about_y(90.0)],
        Interpolation::Linear,
    )
    .expect("a track");
    let scale = Track::new(
        vec![0.0, 1.0],
        vec![[1.0; 3], [3.0; 3]],
        Interpolation::Linear,
    )
    .expect("a track");
    let clip = Clip::new(
        Some("spin".into()),
        vec![
            (1, Channel::Rotation(rotation)),
            (1, Channel::Scale(scale)),
            // A node that the model does not have is left out
            (
                9,
                Channel::Scale(
                    Track::new(vec![0.0], vec![[2.0; 3]], Interpolation::Step).expect("a track"),
                ),
            ),
        ],
    );
    assert_eq!(clip.duration, 4.0);
    let mut nodes = [
        Trs::IDENTITY,
        Trs {
            translation: [5.0, 6.0, 7.0],
            ..Trs::IDENTITY
        },
    ];
    clip.apply(2.0, &mut nodes);
    assert_eq!(nodes[0], Trs::IDENTITY);
    // The translation stays as the file has it
    assert_eq!(nodes[1].translation, [5.0, 6.0, 7.0]);
    near(nodes[1].rotation, about_y(45.0));
    near(nodes[1].scale, [3.0; 3]);
}
