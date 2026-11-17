// Makes `assets/foveated_test.h264`: a test picture that a host compressed the way ALVR's foveated
// encoding does, for a streamed scene to unwarp. Each frame holds both eyes side by side. Each eye
// is the full view of 640 by 720 pixels with a grid of equal squares, a circle and a colored
// square in each corner (red at the top left). After the headset unwarps the picture, the squares
// are equal, the circle is round and the red square lies at the top left of both eyes. The right
// eye is stored mirrored, as ALVR stores it.
//
// The alignment and the warp are ported from ALVR v20.14.1 (`FFR.cpp` and
// `CompressAxisAlignedPixelShader.hlsl`) as cranvr's `foveation.rs` holds them. The program prints
// the aligned values, which `src/foveated_clip.rs` holds as constants. It needs `ffmpeg` with
// libx264 on the path:
//
//     cargo run -p xr-demo --example foveated_clip --no-default-features

#![expect(
    clippy::print_stdout,
    reason = "a command line tool that prints the aligned values for the demo"
)]

use std::{
    io::Write,
    process::{Command, Stdio},
};

const FULL: [u32; 2] = [640, 720];
const CENTER_SIZE: [f32; 2] = [0.45, 0.4];
const CENTER_SHIFT: [f32; 2] = [0.4, 0.1];
const EDGE_RATIO: [f32; 2] = [4.0, 5.0];
const FRAMES: usize = 30;
const OUTPUT: &str = "apps/xr-demo/assets/foveated_test.h264";
// The grid's spacing and line width in pixels of the full view
const CELL: f32 = 80.0;
const LINE: f32 = 6.0;

#[derive(Debug)]
struct Foveation {
    center_size: [f32; 2],
    center_shift: [f32; 2],
    // The encoded eye's size, padded to a multiple of 32 pixels
    padded: [u32; 2],
    eye_size_ratio: [f32; 2],
}

impl Foveation {
    fn new() -> Self {
        let mut f = Self {
            center_size: [0.0; 2],
            center_shift: [0.0; 2],
            padded: [0; 2],
            eye_size_ratio: [0.0; 2],
        };
        for i in 0..2 {
            let t = FULL[i] as f32;
            let er = EDGE_RATIO[i];
            let edge_size = t - CENTER_SIZE[i] * t;
            let center = 1.0 - (edge_size / (er * 2.0)).ceil() * (er * 2.0) / t;
            let edge = t - center * t;
            let shift = (CENTER_SHIFT[i] * edge / (er * 2.0)).ceil() * (er * 2.0) / edge;
            let optimized = (center + (1.0 - center) / er) * t;
            let padded = (optimized / 32.0).ceil() * 32.0;
            f.center_size[i] = center;
            f.center_shift[i] = shift;
            f.padded[i] = padded as u32;
            f.eye_size_ratio[i] = optimized / padded;
        }
        f
    }

    // The warp: the point of the full view that the point `encoded` of the padded encoded eye
    // shows, in the eye's own space with its outer edge at 0.
    fn warp(&self, encoded: [f32; 2]) -> [f32; 2] {
        [0, 1].map(|i| {
            let (cs, sh, er) = (self.center_size[i], self.center_shift[i], EDGE_RATIO[i]);
            let c0 = (1.0 - cs) * 0.5;
            let c1 = (er - 1.0) * c0 * (sh + 1.0) / er;
            let c2 = (er - 1.0) * cs + 1.0;
            let e = encoded[i] / self.eye_size_ratio[i];
            let lo = c0 * (sh + 1.0) / c2;
            let hi = c0 * (sh - 1.0) / c2 + 1.0;
            let center = e * c2 / er + c1;
            if e < lo {
                let g = e / lo;
                g * center + (1.0 - g) * e * c2
            } else if e > hi {
                let g = (1.0 - e) / (1.0 - hi);
                g * center + (1.0 - g) * ((e - 1.0) * c2 + 1.0)
            } else {
                center
            }
        })
    }
}

// The test picture at the point `view` of the full view, as red, green and blue.
fn picture(view: [f32; 2]) -> [f32; 3] {
    if view.iter().any(|v| !(0.0..=1.0).contains(v)) {
        return [0.0; 3];
    }
    let (x, y) = (view[0] * FULL[0] as f32, view[1] * FULL[1] as f32);
    let corner = |left: bool, top: bool| {
        let dx = if left { x } else { FULL[0] as f32 - x };
        let dy = if top { y } else { FULL[1] as f32 - y };
        dx < 70.0 && dy < 70.0
    };
    if corner(true, true) {
        return [230.0, 40.0, 40.0];
    }
    if corner(false, true) {
        return [40.0, 200.0, 60.0];
    }
    if corner(true, false) {
        return [50.0, 90.0, 240.0];
    }
    if corner(false, false) {
        return [240.0, 220.0, 50.0];
    }
    let radius = ((x - 320.0).powi(2) + (y - 360.0).powi(2)).sqrt();
    if (radius - 150.0).abs() < LINE / 2.0 {
        return [255.0, 150.0, 30.0];
    }
    let on_line = |v: f32| (v + LINE / 2.0).rem_euclid(CELL) < LINE;
    if on_line(x) || on_line(y) {
        return [200.0, 200.0, 210.0];
    }
    [30.0, 32.0, 40.0]
}

// The frame: the left eye, then the right eye mirrored, as rows of red, green and blue bytes. Each
// pixel averages 2 by 2 samples.
fn frame(f: &Foveation) -> Vec<u8> {
    let [w, h] = f.padded.map(|v| v as usize);
    let mut bytes = vec![0_u8; 2 * w * h * 3];
    for eye in 0..2 {
        for py in 0..h {
            for px in 0..w {
                let mut sum = [0.0_f32; 3];
                for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let u = (px as f32 + dx) / w as f32;
                    let v = (py as f32 + dy) / h as f32;
                    // The right eye's picture starts at the rectangle's right edge
                    let encoded = [if eye == 1 { 1.0 - u } else { u }, v];
                    let [vx, vy] = f.warp(encoded);
                    let view = [if eye == 1 { 1.0 - vx } else { vx }, vy];
                    for (total, value) in sum.iter_mut().zip(picture(view)) {
                        *total += value / 4.0;
                    }
                }
                let at = (py * 2 * w + eye * w + px) * 3;
                bytes[at..at + 3].copy_from_slice(&sum.map(|v| v.round() as u8));
            }
        }
    }
    bytes
}

fn main() {
    let f = Foveation::new();
    println!("{f:#?}");
    let (width, height) = (2 * f.padded[0], f.padded[1]);
    let mut encoder = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "rgb24"])
        .args(["-s", &format!("{width}x{height}"), "-r", "30", "-i", "-"])
        .args([
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "14", "-bf", "0",
        ])
        .args([
            "-g",
            "30",
            "-x264-params",
            "aud=1:repeat-headers=1:scenecut=0",
        ])
        .args(["-frames:v", &FRAMES.to_string(), "-f", "h264", OUTPUT])
        .stdin(Stdio::piped())
        .spawn()
        .expect("ffmpeg starts");
    let bytes = frame(&f);
    let mut input = encoder.stdin.take().expect("ffmpeg's input");
    for _ in 0..FRAMES {
        input.write_all(&bytes).expect("ffmpeg takes the frame");
    }
    drop(input);
    assert!(encoder.wait().expect("ffmpeg ends").success());
    println!("wrote {OUTPUT}: {width}x{height}");
}
