//! The keyframes that move the nodes of a glTF model.
//!
//! A clip holds channels. Each channel keys the translation, the rotation or the scale of one node
//! over time. Sampling a clip at a time sets those parts of the node transforms it keys. Morph
//! target weights are not keyed here, because the model has no morph targets.

// The transform of a node, as glTF stores it: scale first, then rotation, then translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Trs {
    pub(crate) translation: [f32; 3],
    // A unit quaternion: x, y, z, w
    pub(crate) rotation: [f32; 4],
    pub(crate) scale: [f32; 3],
}

impl Trs {
    pub(crate) const IDENTITY: Self = Self {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };
}

// How a track runs between two keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interpolation {
    Linear,
    // The value of the earlier key holds until the next key
    Step,
    // A Hermite curve. Each key holds an in-tangent, a value and an out-tangent, in that order.
    CubicSpline,
}

// The values of one property of one node at the times of its keys.
#[derive(Clone, Debug)]
pub(crate) struct Track<const N: usize> {
    times: Vec<f32>,
    values: Vec<[f32; N]>,
    interpolation: Interpolation,
}

// Where a time falls among the keys of a track.
enum Place {
    Before,
    After,
    // Between key `key` and the next, `fraction` of the way
    Between { key: usize, fraction: f32 },
}

impl<const N: usize> Track<N> {
    // The track of `times` and `values`, or why they do not make one: a track needs a key, and
    // times that rise.
    pub(crate) fn new(
        times: Vec<f32>,
        values: Vec<[f32; N]>,
        interpolation: Interpolation,
    ) -> Result<Self, String> {
        let per_key = if interpolation == Interpolation::CubicSpline {
            3
        } else {
            1
        };
        if times.is_empty() {
            return Err("a sampler has no keys".into());
        }
        if values.len() != times.len() * per_key {
            return Err(format!(
                "{} keys come with {} values, not {}",
                times.len(),
                values.len(),
                times.len() * per_key
            ));
        }
        if times.windows(2).any(|pair| pair[0] >= pair[1]) || times.iter().any(|t| !t.is_finite()) {
            return Err("the key times do not rise".into());
        }
        Ok(Self {
            times,
            values,
            interpolation,
        })
    }

    // The time of the last key.
    pub(crate) fn end(&self) -> f32 {
        self.times.last().copied().unwrap_or(0.0)
    }

    fn place(&self, time: f32) -> Place {
        let last = self.times.len() - 1;
        if time <= self.times[0] {
            return Place::Before;
        }
        if time >= self.times[last] {
            return Place::After;
        }
        let key = self.times.partition_point(|&key| key <= time) - 1;
        let span = self.times[key + 1] - self.times[key];
        Place::Between {
            key,
            fraction: (time - self.times[key]) / span,
        }
    }

    // The value of key `key`.
    fn value(&self, key: usize) -> [f32; N] {
        match self.interpolation {
            Interpolation::CubicSpline => self.values[key * 3 + 1],
            Interpolation::Linear | Interpolation::Step => self.values[key],
        }
    }

    // The value of key `key`, or `None` past the last key.
    fn key(&self, key: usize) -> Option<[f32; N]> {
        (key < self.times.len()).then(|| self.value(key))
    }

    // The value at `time`. `blend` mixes two values: `fraction` of the way from the first to the
    // second.
    fn sample(&self, time: f32, blend: fn(&[f32; N], &[f32; N], f32) -> [f32; N]) -> [f32; N] {
        match self.place(time) {
            Place::Before => self.value(0),
            Place::After => self.value(self.times.len() - 1),
            Place::Between { key, fraction } => match self.interpolation {
                Interpolation::Step => self.value(key),
                Interpolation::Linear => blend(&self.value(key), &self.value(key + 1), fraction),
                Interpolation::CubicSpline => {
                    let span = self.times[key + 1] - self.times[key];
                    let out_tangent = &self.values[key * 3 + 2];
                    let in_tangent = &self.values[(key + 1) * 3];
                    hermite(
                        [
                            &self.value(key),
                            out_tangent,
                            &self.value(key + 1),
                            in_tangent,
                        ],
                        span,
                        fraction,
                    )
                }
            },
        }
    }
}

pub(crate) fn lerp<const N: usize>(from: &[f32; N], to: &[f32; N], fraction: f32) -> [f32; N] {
    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * fraction)
}

// The turn that lies `fraction` of the way from `from` to `to` along the shorter arc.
pub(crate) fn slerp(from: &[f32; 4], to: &[f32; 4], fraction: f32) -> [f32; 4] {
    let mut dot: f32 = (0..4).map(|i| from[i] * to[i]).sum();
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    dot *= sign;
    // Near the same turn the arc and the chord agree, and the sine in the divisor would vanish
    let (from_weight, to_weight) = if dot > 0.9995 {
        (1.0 - fraction, fraction)
    } else {
        let angle = dot.acos();
        let sine = angle.sin();
        (
            ((1.0 - fraction) * angle).sin() / sine,
            (fraction * angle).sin() / sine,
        )
    };
    std::array::from_fn(|i| from[i] * from_weight + to[i] * sign * to_weight)
}

// The cubic Hermite curve through `[start, out_tangent, end, in_tangent]`, where the tangents are
// per second of `span` and `fraction` runs from 0 to 1.
fn hermite<const N: usize>(
    [start, out_tangent, end, in_tangent]: [&[f32; N]; 4],
    span: f32,
    fraction: f32,
) -> [f32; N] {
    let (t2, t3) = (fraction * fraction, fraction * fraction * fraction);
    let start_weight = 2.0 * t3 - 3.0 * t2 + 1.0;
    let out_weight = (t3 - 2.0 * t2 + fraction) * span;
    let end_weight = -2.0 * t3 + 3.0 * t2;
    let in_weight = (t3 - t2) * span;
    std::array::from_fn(|i| {
        start_weight * start[i]
            + out_weight * out_tangent[i]
            + end_weight * end[i]
            + in_weight * in_tangent[i]
    })
}

// The quaternion scaled to length one, or no turn for a quaternion with no length.
pub(crate) fn normalized_rotation(rotation: [f32; 4]) -> [f32; 4] {
    let length = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length > f32::EPSILON {
        rotation.map(|v| v / length)
    } else {
        Trs::IDENTITY.rotation
    }
}

// One property of one node, keyed over time.
#[derive(Clone, Debug)]
pub(crate) enum Channel {
    Translation(Track<3>),
    Rotation(Track<4>),
    Scale(Track<3>),
}

impl Channel {
    fn end(&self) -> f32 {
        match self {
            Self::Translation(track) | Self::Scale(track) => track.end(),
            Self::Rotation(track) => track.end(),
        }
    }

    fn apply(&self, time: f32, node: &mut Trs) {
        match self {
            Self::Translation(track) => node.translation = track.sample(time, lerp),
            Self::Rotation(track) => node.rotation = normalized_rotation(track.sample(time, slerp)),
            Self::Scale(track) => node.scale = track.sample(time, lerp),
        }
    }

    // Sets the keyed part of `node` to its value at key `key`, or returns `None` when the track has
    // fewer keys.
    fn apply_key(&self, key: usize, node: &mut Trs) -> Option<()> {
        match self {
            Self::Translation(track) => node.translation = track.key(key)?,
            Self::Rotation(track) => node.rotation = normalized_rotation(track.key(key)?),
            Self::Scale(track) => node.scale = track.key(key)?,
        }
        Some(())
    }
}

// The animation of a model: what each of its channels does to its node.
#[derive(Clone, Debug)]
pub(crate) struct Clip {
    pub(crate) name: Option<String>,
    // In seconds: the time of the latest key of any channel
    pub(crate) duration: f32,
    channels: Vec<(usize, Channel)>,
}

impl Clip {
    pub(crate) fn new(name: Option<String>, channels: Vec<(usize, Channel)>) -> Self {
        let duration = channels
            .iter()
            .map(|(_, channel)| channel.end())
            .fold(0.0, f32::max);
        Self {
            name,
            duration,
            channels,
        }
    }

    // The transform of `node` at key `key` of each channel that keys it, over `rest` for the parts
    // that no channel keys. A key is a place in a channel's list of keys, not a time: a model can
    // keep a pose for each input of a device at a fixed key. `None` when no channel keys the node,
    // or when a channel that keys it has no such key.
    pub(crate) fn pose_at_key(&self, node: usize, key: usize, rest: Trs) -> Option<Trs> {
        let mut pose = rest;
        let mut keyed = false;
        for (target, channel) in &self.channels {
            if *target == node {
                channel.apply_key(key, &mut pose)?;
                keyed = true;
            }
        }
        keyed.then_some(pose)
    }

    // Sets the keyed parts of the nodes' transforms to their values `time` seconds into the clip.
    // A node that the model does not have is left out.
    pub(crate) fn apply(&self, time: f32, nodes: &mut [Trs]) {
        for (node, channel) in &self.channels {
            if let Some(node) = nodes.get_mut(*node) {
                channel.apply(time, node);
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/gltf_clip_tests.rs"]
mod tests;
