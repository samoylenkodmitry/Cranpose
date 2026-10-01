use std::{path::PathBuf, time::Duration};

use cranpose::{AppLauncher, RobotScreenshot, RobotTimelineAction, RobotTimelineStep};

use crate::{
    liquid_control_reference::{Control, LiquidControlReference},
    liquid_tab_reference::ReferenceBackdrop,
    robot_exit, robot_shot,
};

fn contrast(frame: &RobotScreenshot, y: usize) -> u8 {
    let mut edge = 0;
    for x in 190 * 3..212 * 3 {
        let index = (y * 3 * frame.width as usize + x) * 4;
        for channel in 0..3 {
            edge = edge
                .max(frame.pixels[index + channel].abs_diff(frame.pixels[index + 3 * 4 + channel]));
        }
    }
    edge
}

pub(crate) fn main() -> anyhow::Result<()> {
    let name = std::env::var("REFERENCE_COMPONENT").unwrap_or_else(|_| "button".into());
    let control = Control::parse(&name)?;
    let backdrop = ReferenceBackdrop::parse(
        &std::env::var("REFERENCE_BACKDROP").unwrap_or_else(|_| "checkerboard-mono".into()),
    )?;
    let initial = std::env::var("REFERENCE_VALUE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0);
    let face_expected = std::env::var("REFERENCE_FACE_EXPECTED")
        .ok()
        .map(|value| serde_json::from_str::<[u8; 3]>(&value))
        .transpose()?;
    let output = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/liquid-floating-optics".into()),
    );
    std::fs::create_dir_all(&output)?;
    AppLauncher::new()
        .with_title("Floating glass checker transmission")
        .with_size(402, 874)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fonts_from(desktop_app::fonts::register_liquid_reference_fonts)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(120);
            std::thread::sleep(Duration::from_millis(500));
            let steps = capture_steps();
            let frames = robot
                .capture_interaction_keyframes(3.0, &steps)
                .expect("capture floating glass states");
            for (frame, name) in frames.iter().zip([
                "rest.png",
                "held.png",
                "release-50ms.png",
                "release-100ms.png",
                "release-250ms.png",
                "release-500ms.png",
                "released.png",
            ]) {
                robot_shot::save_checked(&output.join(name), frame)
                    .expect("save floating glass state");
            }
            if std::env::var_os("CRANPOSE_GPU_PASS_TIMING").is_some() {
                let steps = motion_steps();
                for leg in 0..24 {
                    let measured = robot
                        .capture_interaction_keyframes(3.0, &steps)
                        .expect("measure floating glass animation");
                    let changing = measured
                        .windows(2)
                        .filter(|pair| pair[0].pixels != pair[1].pixels)
                        .count();
                    println!("robot-metric: floating leg={leg} changing_frames={changing}");
                    assert!(
                        changing >= 20,
                        "benchmark must render changing press and release frames"
                    );
                }
            }
            if let Some(expected) = face_expected {
                let sample = robot_shot::logical_sampler(&frames[0])(201.0, 463.0);
                let actual = [sample.0, sample.1, sample.2];
                println!("robot-metric: native_face expected={expected:?} actual={actual:?}");
                assert!(
                    actual
                        .into_iter()
                        .zip(expected)
                        .all(|(a, e)| a.abs_diff(e) <= 4),
                    "flat-field native face transfer: expected={expected:?} actual={actual:?}"
                );
            } else {
                assert_optics(&frames, control, initial, backdrop);
            }
            if control == Control::FilterChip {
                assert_chip_reversals(&robot, &output);
            }
            robot.exit().expect("exit floating glass regression");
        })
        .try_run(move || {
            cranpose_core::CompositionLocalProvider(
                [cranpose::local_safe_area_insets()
                    .provides(cranpose::EdgeInsets::from_components(0.0, 62.0, 0.0, 34.0))],
                move || LiquidControlReference(control, backdrop, false, initial),
            );
        })?;
    Ok(())
}

fn assert_chip_reversals(robot: &crate::RobotHandle, output: &std::path::Path) {
    let mut steps = Vec::new();
    for elapsed in [0, 1, 3, 9, 21] {
        steps.push(RobotTimelineStep {
            advance_ms: 0.0,
            actions: vec![RobotTimelineAction::MouseDown, RobotTimelineAction::MouseUp],
            capture: false,
        });
        wait_and_capture(&mut steps, elapsed);
        steps.push(RobotTimelineStep {
            advance_ms: 0.0,
            actions: vec![RobotTimelineAction::MouseDown, RobotTimelineAction::MouseUp],
            capture: true,
        });
        wait_and_capture(&mut steps, 36);
    }
    let reversals = robot
        .capture_interaction_keyframes(3.0, &steps)
        .expect("capture interrupted chip transitions");
    for (index, pair) in reversals.chunks_exact(3).enumerate() {
        for (frame, suffix) in pair.iter().zip(["before", "after", "settled"]) {
            robot_shot::save_checked(
                &output.join(format!("reversal-{index}-{suffix}.png")),
                frame,
            )
            .expect("save chip reversal");
        }
        let changed: usize = (390 * 3..515 * 3)
            .map(|y| {
                let start = (y * pair[0].width as usize + 130 * 3) * 4;
                let end = start + 142 * 3 * 4;
                pair[0].pixels[start..end]
                    .iter()
                    .zip(&pair[1].pixels[start..end])
                    .filter(|(before, after)| before.abs_diff(**after) > 1)
                    .count()
            })
            .sum();
        println!("robot-metric: chip_reversal={index} changed_channels={changed}");
        assert_eq!(
            changed, 0,
            "retargeting a chip without advancing time must preserve its rendered appearance"
        );
    }
}

fn assert_optics(
    frames: &[RobotScreenshot],
    control: Control,
    initial: f32,
    backdrop: ReferenceBackdrop,
) {
    if matches!(
        control,
        Control::Button | Control::Chip | Control::FilterChip
    ) && initial == 0.0
    {
        let y = if control == Control::Button { 468 } else { 462 };
        let rest = contrast(&frames[0], y);
        let held = contrast(&frames[1], y);
        println!("robot-metric: transmitted_checker_contrast rest={rest} held={held}");
        let minimum = if backdrop == ReferenceBackdrop::Rainbow {
            20
        } else {
            15
        };
        assert!(rest >= minimum && held >= minimum,
                "native floating glass retains distinct checker cells beneath the label, rest={rest} held={held}");
    }
    if control == Control::FilterChip {
        let selected = &frames[if initial == 0.0 { 6 } else { 0 }];
        let sample = robot_shot::logical_sampler(selected)(201.0, 462.0);
        assert!(
            sample.2.saturating_sub(sample.0) >= 80,
            "selected native filter chip has an accent-filled glass body: {sample:?}"
        );
        if initial > 0.0 {
            let early = robot_shot::logical_sampler(&frames[2])(201.0, 462.0);
            let settled = robot_shot::logical_sampler(&frames[6])(201.0, 462.0);
            assert!(early.2.saturating_sub(early.0) >= 80,
                "native selected chip retains its transmitted blue while deselection begins: {early:?}");
            assert!(
                backdrop != ReferenceBackdrop::Monochrome || settled.2.abs_diff(settled.0) < 10,
                "deselection must settle to clear glass: {settled:?}"
            );
        }
    }
    if control == Control::ProminentButton || (control == Control::FilterChip && initial > 0.0) {
        let y = if control == Control::ProminentButton {
            468
        } else {
            462
        };
        let rest = contrast(&frames[0], y);
        let held = contrast(&frames[1], y);
        let minimum = if backdrop == ReferenceBackdrop::Rainbow {
            2
        } else {
            5
        };
        assert!(
            rest >= minimum && held >= minimum,
            "colored glass must transmit checker structure, rest={rest} held={held}"
        );
        let rest_color = robot_shot::logical_sampler(&frames[0])(201.0, y as f32);
        let held_color = robot_shot::logical_sampler(&frames[1])(201.0, y as f32);
        assert!(held_color.0 > rest_color.0.saturating_add(40),
            "white contact light must illuminate blue glass: rest={rest_color:?}, held={held_color:?}");
    }
}

fn motion_steps() -> Vec<RobotTimelineStep> {
    (0..60)
        .map(|frame| RobotTimelineStep {
            advance_ms: 1000.0 / 60.0,
            actions: match frame {
                0 => vec![
                    RobotTimelineAction::MoveTo { x: 201.0, y: 451.0 },
                    RobotTimelineAction::MouseDown,
                ],
                30 => vec![RobotTimelineAction::MouseUp],
                _ => Vec::new(),
            },
            capture: true,
        })
        .collect()
}

fn capture_steps() -> Vec<RobotTimelineStep> {
    let mut steps = vec![
        RobotTimelineStep {
            advance_ms: 0.0,
            actions: Vec::new(),
            capture: true,
        },
        RobotTimelineStep {
            advance_ms: 0.0,
            actions: vec![
                RobotTimelineAction::MoveTo { x: 201.0, y: 451.0 },
                RobotTimelineAction::MouseDown,
            ],
            capture: false,
        },
    ];
    wait_and_capture(&mut steps, 36);
    steps.push(RobotTimelineStep {
        advance_ms: 0.0,
        actions: vec![RobotTimelineAction::MouseUp],
        capture: false,
    });
    for frames in [3, 3, 9, 15, 90] {
        wait_and_capture(&mut steps, frames);
    }
    steps
}

fn wait_and_capture(steps: &mut Vec<RobotTimelineStep>, frames: usize) {
    steps.extend((0..frames).map(|_| RobotTimelineStep {
        advance_ms: 1000.0 / 60.0,
        actions: Vec::new(),
        capture: false,
    }));
    steps.push(RobotTimelineStep {
        advance_ms: 0.0,
        actions: Vec::new(),
        capture: true,
    });
}
