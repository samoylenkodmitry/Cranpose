//! The parts of a controller's glTF model that move with its inputs: the trigger, the grip, the
//! face buttons, the menu button and the thumbstick.
//!
//! A Meta Quest's runtime hands out its controllers' models through `XR_FB_render_model` with these
//! parts as nodes. A node's name holds the name of its input, such as `b_trigger_front` or
//! `b_button_x`. The file's first animation keeps the pose of each part at a fixed key, which is a
//! place in the list of keys and not a time: the trigger pulled at key 16, the grip squeezed at key
//! 21, the first face button down at key 5, the second at key 8, the menu button down at key 24, and
//! the thumbstick leaned to the eight points of the compass at keys 29, 39, 34, 40, 31, 36, 32 and
//! 37, starting forward and going to the right. Meta's Unity SDK reads the same keys
//! (`OVRGLTFAnimatinonNode`). The node's own transform is the part at rest.
//!
//! A part moves only in a model that has its node and a pose at its key that differs from the
//! rest. Any model that follows this layout moves its parts, whoever made it. A model with none of
//! these nodes has no parts and costs nothing.

#[cfg(any(test, headset))]
use crate::Controller;
use crate::gltf_clip::{Clip, Trs, lerp, normalized_rotation, slerp};

// The key of the full press of each input, and of the thumbstick's lean to each point of the
// compass, from forward going to the right.
const TRIGGER_KEY: usize = 16;
const SQUEEZE_KEY: usize = 21;
const PRIMARY_KEY: usize = 5;
const SECONDARY_KEY: usize = 8;
const MENU_KEY: usize = 24;
const LEAN_KEYS: [usize; 8] = [29, 39, 34, 40, 31, 36, 32, 37];

// A transform closer than this to the rest pose in every number is the rest pose: the file then
// holds no pose for the part.
const SAME: f32 = 1.0e-5;

// The input that presses a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    Trigger,
    Squeeze,
    Primary,
    Secondary,
    Menu,
}

impl Press {
    // The press of each input, by the words that its node's name holds, in the order that a name is
    // tested in. The menu button's node is named for the Meta button: the left controller has the
    // menu button there. The right controller has the Meta button, which a controller reports to no
    // app, so its node stays at rest.
    const NAMES: [(&'static str, Self); 7] = [
        ("trigger_front", Self::Trigger),
        ("trigger_grip", Self::Squeeze),
        ("button_a", Self::Primary),
        ("button_x", Self::Primary),
        ("button_b", Self::Secondary),
        ("button_y", Self::Secondary),
        ("button_oculus", Self::Menu),
    ];

    fn key(self) -> usize {
        match self {
            Self::Trigger => TRIGGER_KEY,
            Self::Squeeze => SQUEEZE_KEY,
            Self::Primary => PRIMARY_KEY,
            Self::Secondary => SECONDARY_KEY,
            Self::Menu => MENU_KEY,
        }
    }

    // How far the input presses its part, from 0 to 1.
    fn value(self, input: &PartInput) -> f32 {
        match self {
            Self::Trigger => input.trigger,
            Self::Squeeze => input.squeeze,
            Self::Primary => f32::from(u8::from(input.primary)),
            Self::Secondary => f32::from(u8::from(input.secondary)),
            Self::Menu => f32::from(u8::from(input.menu)),
        }
    }
}

// How a part moves.
#[derive(Clone, Debug, PartialEq)]
enum Motion {
    // From the rest pose to `pressed` as the input goes from 0 to 1
    Press { input: Press, pressed: Trs },
    // To the lean of each point of the compass, from forward going to the right
    Lean(Box<[Trs; 8]>),
}

#[derive(Clone, Debug, PartialEq)]
struct Part {
    node: usize,
    rest: Trs,
    motion: Motion,
}

// What the inputs of a controller do to its model's parts. Two inputs that are equal pose the parts
// the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PartInput {
    trigger: f32,
    squeeze: f32,
    primary: bool,
    secondary: bool,
    menu: bool,
    // The thumbstick's lean to the right and forward
    stick: [f32; 2],
}

impl PartInput {
    #[cfg(any(test, headset))]
    pub(crate) fn of(controller: &Controller) -> Self {
        Self {
            trigger: controller.trigger.value,
            squeeze: controller.squeeze.value,
            primary: controller.primary.pressed,
            secondary: controller.secondary.pressed,
            menu: controller.menu,
            stick: [controller.thumbstick.x, controller.thumbstick.y],
        }
    }
}

// The parts of a model that move with a controller's inputs.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ControllerParts {
    parts: Vec<Part>,
}

impl ControllerParts {
    // The parts of a model whose nodes have `names` and `rest` transforms, posed by its first clip.
    pub(crate) fn find(
        names: &[Option<std::sync::Arc<str>>],
        rest: &[Trs],
        clips: &[Clip],
    ) -> Self {
        let Some(clip) = clips.first() else {
            return Self::default();
        };
        let parts = names
            .iter()
            .enumerate()
            .filter_map(|(node, name)| {
                let name = name.as_deref()?;
                let rest = rest[node];
                let motion = motion_of(name, |key| {
                    clip.pose_at_key(node, key, rest)
                        .filter(|pose| !same(pose, &rest))
                })?;
                Some(Part { node, rest, motion })
            })
            .collect();
        Self { parts }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    // The names of the inputs whose parts move, for the log.
    pub(crate) fn describe(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.parts.iter().map(|part| match part.motion {
            Motion::Press { input, .. } => match input {
                Press::Trigger => "trigger",
                Press::Squeeze => "grip",
                Press::Primary => "first button",
                Press::Secondary => "second button",
                Press::Menu => "menu button",
            },
            Motion::Lean(_) => "thumbstick",
        })
    }

    // Writes the transform of each part that `input` gives into `locals`, indexed by node.
    pub(crate) fn pose(&self, input: &PartInput, locals: &mut [Trs]) {
        for part in &self.parts {
            let Some(local) = locals.get_mut(part.node) else {
                continue;
            };
            *local = match &part.motion {
                Motion::Press {
                    input: press,
                    pressed,
                } => mix(&part.rest, pressed, press.value(input).clamp(0.0, 1.0)),
                Motion::Lean(leans) => lean(&part.rest, leans, input.stick),
            };
        }
    }
}

// How the part whose node is called `name` moves, given the pose of each key, or `None` for a node
// that is no part or whose file holds no pose for it.
fn motion_of(name: &str, pose_at: impl Fn(usize) -> Option<Trs>) -> Option<Motion> {
    if name.contains("thumbstick") {
        let mut leans = [Trs::IDENTITY; 8];
        for (lean, key) in leans.iter_mut().zip(LEAN_KEYS) {
            *lean = pose_at(key)?;
        }
        return Some(Motion::Lean(Box::new(leans)));
    }
    let (_, input) = Press::NAMES
        .into_iter()
        .find(|(words, _)| name.contains(words))?;
    Some(Motion::Press {
        input,
        pressed: pose_at(input.key())?,
    })
}

fn same(a: &Trs, b: &Trs) -> bool {
    let close = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(a, b)| (a - b).abs() <= SAME);
    // A quaternion and its negation are the same turn
    let negated = b.rotation.map(|value| -value);
    close(&a.translation, &b.translation)
        && close(&a.scale, &b.scale)
        && (close(&a.rotation, &b.rotation) || close(&a.rotation, &negated))
}

// The transform `fraction` of the way from `from` to `to`.
fn mix(from: &Trs, to: &Trs, fraction: f32) -> Trs {
    Trs {
        translation: lerp(&from.translation, &to.translation, fraction),
        rotation: normalized_rotation(slerp(&from.rotation, &to.rotation, fraction)),
        scale: lerp(&from.scale, &to.scale, fraction),
    }
}

// The thumbstick's pose for a lean of `stick` (right, forward). The lean's direction falls between
// two points of the compass: the pose between their keys follows the angle. The lean's length then
// takes the part that far from the rest pose. A lean longer than 1 counts as 1.
fn lean(rest: &Trs, leans: &[Trs; 8], [right, forward]: [f32; 2]) -> Trs {
    let length = right.hypot(forward);
    if !length.is_finite() || length <= f32::EPSILON {
        return *rest;
    }
    // Eighths of a turn from forward toward the right
    let eighths =
        right.atan2(forward).rem_euclid(std::f32::consts::TAU) * 8.0 / std::f32::consts::TAU;
    let from = (1u8..8)
        .rev()
        .find(|&point| eighths >= f32::from(point))
        .unwrap_or(0);
    let fraction = eighths - f32::from(from);
    let from = usize::from(from);
    let toward = mix(&leans[from], &leans[(from + 1) % 8], fraction);
    mix(rest, &toward, length.min(1.0))
}

#[cfg(test)]
#[path = "tests/controller_parts_tests.rs"]
mod tests;
