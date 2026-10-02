use crate::{robot_exit, robot_shot};
use cranpose::{
    liquid::prelude::*, widgets::{Box, BoxSpec}, AppLauncher, Color, Modifier,
    RobotTimelineAction, RobotTimelineStep,
};
use std::{path::PathBuf, time::Duration};

fn hold(steps: &mut Vec<RobotTimelineStep>, frames: usize) {
    steps.extend((0..frames).map(|_| RobotTimelineStep {
        advance_ms: 1000.0 / 60.0,
        actions: Vec::new(),
        capture: false,
    }));
}

fn measure_motion(robot: &cranpose::Robot) {
    if std::env::var_os("CRANPOSE_GPU_PASS_TIMING").is_some() {
        for leg in 0..24 {
            let steps: Vec<_> = (0..60).map(|frame| {
                let progress = frame as f32 / 59.0;
                let progress = if leg % 2 == 0 { progress } else { 1.0 - progress };
                RobotTimelineStep {
                    advance_ms: 1000.0 / 60.0,
                    actions: vec![RobotTimelineAction::MoveTo { x: 101.0 + progress * 200.0, y: 60.0 }],
                    capture: true,
                }
            }).collect();
            let measured = robot.capture_interaction_keyframes(3.0, &steps).expect("measure active lens frames");
            assert_eq!(measured.len(), steps.len());
            let changing = measured.windows(2).filter(|pair| pair[0].pixels != pair[1].pixels).count();
            println!("robot-metric: active lens leg={leg} changing_frames={changing}");
            assert!(changing >= 50, "GPU measurement must render the moving lens");
        }
    }
}

pub(crate) fn main() -> anyhow::Result<()> {
    let output = PathBuf::from(std::env::var("ROBOT_SHOT_DIR")
        .unwrap_or_else(|_| "target/liquid-control-refraction".to_string()));
    std::fs::create_dir_all(&output)?;
    AppLauncher::new()
        .with_title("Liquid control source mapping")
        .with_size(402, 200)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(120);
            std::thread::sleep(Duration::from_millis(500));
            let mut steps = vec![RobotTimelineStep {
                advance_ms: 0.0,
                actions: Vec::new(),
                capture: true,
            }, RobotTimelineStep {
                advance_ms: 0.0,
                actions: vec![RobotTimelineAction::MoveTo { x: 190.0, y: 150.0 }, RobotTimelineAction::MouseDown],
                capture: false,
            }];
            hold(&mut steps, 31);
            steps.push(RobotTimelineStep { advance_ms: 0.0, actions: Vec::new(), capture: true });
            steps.push(RobotTimelineStep {
                advance_ms: 0.0,
                actions: vec![RobotTimelineAction::MouseUp, RobotTimelineAction::MoveTo { x: 101.0, y: 60.0 }, RobotTimelineAction::MouseDown],
                capture: false,
            });
            hold(&mut steps, 36);
            for frame in 1..=18 {
                steps.push(RobotTimelineStep {
                    advance_ms: 1000.0 / 60.0,
                    actions: vec![RobotTimelineAction::MoveTo { x: 101.0 + 48.0 * frame as f32 / 18.0, y: 60.0 }],
                    capture: false,
                });
            }
            hold(&mut steps, 36);
            steps.push(RobotTimelineStep { advance_ms: 0.0, actions: Vec::new(), capture: true });
            let frames = robot.capture_interaction_keyframes(3.0, &steps).expect("capture held control optics");
            for (frame, name) in frames.iter().zip(["rest.png", "toggle-held.png", "segmented-held.png"]) {
                robot_shot::save(frame, &output, name);
            }
            measure_motion(&robot);
            let red = |frame: usize, x: usize, y: usize| frames[frame].pixels[(y * frames[frame].width as usize + x) * 4];
            let band = red(1, 172 * 3, 150 * 3);
            let center = red(1, 190 * 3, 150 * 3);
            println!("[liquid] switch transmitted band={band} track center={center}");
            assert!(band >= 230 && center < 220,
                    "native held switch refracts the white surround into the broad left band while transmitting the gray track; band={band}, center={center}");
            let mut displaced = 0;
            for y in 52 * 3..68 * 3 {
                for x in 85 * 3..116 * 3 {
                    displaced += usize::from((red(0, x, y) < 5) != (red(2, x, y) < 5));
                }
            }
            println!("[liquid] segmented displaced ink pixels={displaced}");
            assert!(displaced >= 8,
                "native segmented glass samples its label row: a stationary glass edge crossing All must displace ink; changed={displaced}");
            let mut chromatic = 0;
            for y in 38 * 3..82 * 3 {
                for x in 88 * 3..210 * 3 {
                    let index = (y * frames[2].width as usize + x) * 4;
                    let channels = &frames[2].pixels[index..index + 3];
                    let low = channels.iter().copied().min().expect("RGB minimum");
                    let high = channels.iter().copied().max().expect("RGB maximum");
                    chromatic += usize::from(high - low > 16);
                }
            }
            assert!(chromatic >= 40, "native dispersion separates label and track edges into colors; chromatic pixels={chromatic}");
            let rim = (36 * 3..41 * 3).map(|y| red(2, 149 * 3, y)).min().expect("rim scan");
            assert!(rim <= 230, "native contour reflection remains visible against white before dispersion; darkest rim={rim}");
            let opening = red(2, 149 * 3, 84 * 3);
            let ring = red(2, 149 * 3, 90 * 3);
            assert!(opening >= 250 && ring <= 246,
                "native glass casts a contour shadow with a light opening, not a filled drop shadow; opening={opening}, ring={ring}");
            robot.mouse_up().expect("release control");
            robot.exit().expect("exit");
        })
        .try_run(|| {
            LiquidTheme(LiquidThemeSpec { scheme: SchemeMode::Light, ..Default::default() }, || {
                Box(Modifier::empty().fill_max_size().background(Color::WHITE), BoxSpec::default(), || {
                    LiquidToggle(Modifier::empty().absolute_offset(0.0, 136.0).graphics_layer(|| cranpose::GraphicsLayer {
                        translation_x: 170.66667,
                        ..Default::default()
                    }), false, |_| {});
                    LiquidSegmentedControl(Modifier::empty().absolute_offset(51.0, 44.0).width(300.0), 0, |_| {}, |scope| {
                        scope.segment("All");
                        scope.segment("Unread");
                        scope.segment("Saved");
                    });
                });
            });
        })?;
    Ok(())
}
