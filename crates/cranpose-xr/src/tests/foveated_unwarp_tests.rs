use super::*;

// The host's side of the encoding, ported from ALVR v20.14.1 as cranvr's `foveation.rs` holds it:
// it aligns the center to the encoder's blocks, and for each point of the encoded picture it
// says which point of the full view the picture shows there.
struct Host {
    encoding: FoveatedEncoding,
}

impl Host {
    fn new(target: [u32; 2]) -> Self {
        let (config_size, config_shift, edge_ratio) = ([0.45, 0.4], [0.4, 0.1], [4.0, 5.0]);
        let mut encoding = FoveatedEncoding {
            full_size: (target[0], target[1]),
            center_size: [0.0; 2],
            center_shift: [0.0; 2],
            edge_ratio,
            eye_size_ratio: [0.0; 2],
            mirrored: false,
        };
        for i in 0..2 {
            let t = target[i] as f32;
            let er = edge_ratio[i];
            let edge_size = t - config_size[i] * t;
            let center = 1.0 - (edge_size / (er * 2.0)).ceil() * (er * 2.0) / t;
            let edge = t - center * t;
            let shift = (config_shift[i] * edge / (er * 2.0)).ceil() * (er * 2.0) / edge;
            let optimized = (center + (1.0 - center) / er) * t;
            let padded = (optimized / 32.0).ceil() * 32.0;
            encoding.center_size[i] = center;
            encoding.center_shift[i] = shift;
            encoding.eye_size_ratio[i] = optimized / padded;
        }
        Self { encoding }
    }

    // The warp: a point of the padded encoded picture, 0 to 1 on each axis, to the full view.
    fn warp(&self, encoded: [f32; 2]) -> [f32; 2] {
        let e = &self.encoding;
        [0, 1].map(|i| {
            let (cs, sh, er) = (e.center_size[i], e.center_shift[i], e.edge_ratio[i]);
            let c0 = (1.0 - cs) * 0.5;
            let c1 = (er - 1.0) * c0 * (sh + 1.0) / er;
            let c2 = (er - 1.0) * cs + 1.0;
            let x = encoded[i] / e.eye_size_ratio[i];
            let lo = c0 * (sh + 1.0) / c2;
            let hi = c0 * (sh - 1.0) / c2 + 1.0;
            let center = x * c2 / er + c1;
            if x < lo {
                let g = x / lo;
                g * center + (1.0 - g) * x * c2
            } else if x > hi {
                let g = (1.0 - x) / (1.0 - hi);
                g * center + (1.0 - g) * ((x - 1.0) * c2 + 1.0)
            } else {
                center
            }
        })
    }

    // The two points of each axis on both sides of each boundary between the pieces
    fn boundaries(&self, axis: usize) -> [f32; 4] {
        let e = &self.encoding;
        let c0 = (1.0 - e.center_size[axis]) * 0.5;
        let sh = e.center_shift[axis];
        let (lo, hi) = (c0 * (sh + 1.0), c0 * (sh - 1.0) + 1.0);
        [lo - 1e-3, lo + 1e-3, hi - 1e-3, hi + 1e-3]
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-4
}

#[test]
fn unwarping_a_warped_point_returns_the_point() {
    for target in [[1680, 1760], [2064, 2208], [1248, 1312]] {
        let host = Host::new(target);
        let unwarp = FoveatedUnwarp::new(&host.encoding);
        let ratio = host.encoding.eye_size_ratio;
        for i in 1..50 {
            for j in 1..50 {
                // The encoded picture holds the eye in the share `eye_size_ratio` of its rectangle
                let encoded = [i as f32 / 50.0 * ratio[0], j as f32 / 50.0 * ratio[1]];
                let back = unwarp.sample(host.warp(encoded));
                assert!(
                    near(back[0], encoded[0]) && near(back[1], encoded[1]),
                    "{target:?} {encoded:?} came back as {back:?}"
                );
            }
        }
    }
}

#[test]
fn a_point_of_the_view_survives_the_warp_back_and_forth_beside_each_boundary() {
    let host = Host::new([2064, 2208]);
    let unwarp = FoveatedUnwarp::new(&host.encoding);
    let (xs, ys) = (host.boundaries(0), host.boundaries(1));
    let middle = [0.02, 0.5, 0.98];
    for x in xs.into_iter().chain(middle) {
        for y in ys.into_iter().chain(middle) {
            let back = host.warp(unwarp.sample([x, y]));
            assert!(
                near(back[0], x) && near(back[1], y),
                "{x} {y} came back as {back:?}"
            );
        }
    }
}

#[test]
fn a_mirrored_picture_is_read_from_the_far_edge() {
    let mut host = Host::new([2064, 2208]);
    let plain = FoveatedUnwarp::new(&host.encoding);
    host.encoding.mirrored = true;
    let mirrored = FoveatedUnwarp::new(&host.encoding);
    for (x, y) in [(0.1, 0.3), (0.5, 0.5), (0.93, 0.8)] {
        // The mirrored eye's outer edge is the plain eye's outer edge turned around
        let turned = plain.sample([1.0 - x, y]);
        let sample = mirrored.sample([x, y]);
        assert!(
            near(sample[0], 1.0 - turned[0]) && near(sample[1], turned[1]),
            "{x} {y}: {sample:?} against {turned:?}"
        );
    }
    // The right edge of the view is the outer edge. The picture starts there at the rectangle's
    // right edge, and its padding lies on the left of the rectangle.
    let outer = mirrored.sample([1.0, 0.5]);
    assert!(near(outer[0], 1.0), "{outer:?}");
    let padding = 1.0 - host.encoding.eye_size_ratio[0];
    let inner = mirrored.sample([0.0, 0.5]);
    assert!(
        near(inner[0], padding),
        "{inner:?} against padding {padding}"
    );
}

#[test]
fn the_edges_compress_and_the_center_keeps_its_density() {
    let host = Host::new([2064, 2208]);
    let unwarp = FoveatedUnwarp::new(&host.encoding);
    let e = &host.encoding;
    // Encoded pixels that the picture holds for one pixel of the full view, across the width
    let unpadded_share = e.center_size[0] + (1.0 - e.center_size[0]) / e.edge_ratio[0];
    let density = |x: f32| {
        let step = 1e-3;
        let slope = (unwarp.sample([x + step, 0.5])[0] - unwarp.sample([x, 0.5])[0]) / step;
        slope / e.eye_size_ratio[0] * unpadded_share
    };
    let [lo, _, hi, _] = host.boundaries(0);
    let middle = (lo + hi) / 2.0;
    assert!(near_density(density(middle), 1.0), "{}", density(middle));
    assert!(density(0.02) < 0.4, "{}", density(0.02));
}

fn near_density(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.02
}

#[test]
fn a_description_that_the_unwarp_cannot_use_is_refused() {
    let good = Host::new([2064, 2208]).encoding;
    assert_eq!(good.checked(), Ok(()));
    let refused = [
        FoveatedEncoding {
            full_size: (0, 2208),
            ..good
        },
        FoveatedEncoding {
            center_size: [1.0, 0.4],
            ..good
        },
        FoveatedEncoding {
            center_shift: [0.4, f32::NAN],
            ..good
        },
        FoveatedEncoding {
            edge_ratio: [1.0, 5.0],
            ..good
        },
        FoveatedEncoding {
            eye_size_ratio: [1.2, 1.0],
            ..good
        },
    ];
    for encoding in refused {
        assert!(encoding.checked().is_err(), "{encoding:?}");
    }
}
